/**
 * Spire Webview — app.js
 *
 * Tab-based UI with Chat, MCP, Agents, and Configuration tabs.
 * Communicates with the VS Code extension host via acquireVsCodeApi().
 *
 * Message protocol:
 *   Webview → Extension: { type: 'call', method, params, id }
 *   Extension → Webview: { type: 'response', id, result }
 *   Extension → Webview: { type: 'notification', method, params }
 *   Extension → Webview: { type: 'error', message }
 *   Extension → Webview: { type: 'status', connected: bool }
 */

(function () {
  'use strict';

  // ── VS Code API ──────────────────────────────────────────────────────────

  const vscode = acquireVsCodeApi();

  // ── State ─────────────────────────────────────────────────────────────────

  const state = {
    connected: false,
    activeTab: 'chat',
    // Chat state
    messages: [],
    chatId: 'default',
    isProcessing: false,
    pendingRequestId: 0,
    pendingRequests: new Map(),
    // MCP state
    mcpServers: [],
    mcpExpandedServer: null,
    mcpServerTools: {},   // serverName → McpToolInfo[]
    mcpLoading: false,
    // Tools state (real-time tool usage feed)
    toolEvents: [],       // { id, tool_name, status, args, result, error, duration_ms, timestamp }
    toolEventIdCounter: 0,
    maxToolEvents: 200,   // keep last 200 events
    // Build progress tracking — shows build status as a system message in chat
    buildProgressMessageId: null,  // tracks the DOM element id for the build progress message
  };

  // ── DOM References ────────────────────────────────────────────────────────

  const messagesEl = document.getElementById('messages');
  const emptyState = document.getElementById('empty-state');
  const inputEl = document.getElementById('message-input');
  const sendBtn = document.getElementById('send-btn');
  const statusDot = document.getElementById('status-dot');
  const statusText = document.getElementById('status-text');
  const errorBanner = document.getElementById('error-banner');
  const typingIndicator = document.getElementById('typing-indicator');

  // Restore previous state if available (persists across webview reloads)
  const previousState = vscode.getState();
  if (previousState && previousState.messages && previousState.messages.length > 0) {
    state.messages = previousState.messages;
    state.chatId = previousState.chatId || 'default';
    state.activeTab = previousState.activeTab || 'chat';
  }


  // Tab references
  const tabBar = document.getElementById('tab-bar');
  const tabBtns = tabBar.querySelectorAll('.tab-btn');

  // MCP references
  const mcpServerList = document.getElementById('mcp-server-list');
  const mcpEmptyState = document.getElementById('mcp-empty-state');
  const mcpRefreshBtn = document.getElementById('mcp-refresh-btn');

  // ── JSON-RPC Helpers ──────────────────────────────────────────────────────

  let requestIdCounter = 0;

  /**
   * Default timeout per method prefix (in milliseconds).
   * LLM methods get 120s to match the Rust LlmActor's reqwest timeout.
   * Other methods use 30s.
   */
  const METHOD_TIMEOUTS = {
    'llm/': 120_000,
    'chat/': 120_000,
    'default': 30_000,
  };

  /**
   * Get the timeout for a given method.
   */
  function getTimeout(method) {
    for (const [prefix, timeout] of Object.entries(METHOD_TIMEOUTS)) {
      if (method.startsWith(prefix)) {
        return timeout;
      }
    }
    return METHOD_TIMEOUTS['default'];
  }

  /**
   * Send a JSON-RPC call to the extension host.
   * Returns a promise that resolves with the result.
   */
  function call(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++requestIdCounter;
      state.pendingRequests.set(id, { resolve, reject });

      vscode.postMessage({
        type: 'call',
        method,
        params,
        id,
      });

      // Use method-specific timeout (LLM calls can take >60s)
      const timeout = getTimeout(method);
      setTimeout(() => {
        if (state.pendingRequests.has(id)) {
          state.pendingRequests.delete(id);
          reject(new Error(`Request timed out: ${method}`));
        }
      }, timeout);
    });
  }

  /**
   * Send a JSON-RPC notification (fire-and-forget).
   */
  function notify(method, params = {}) {
    vscode.postMessage({
      type: 'notify',
      method,
      params,
    });
  }

  // ── Tab Switching ─────────────────────────────────────────────────────────

  function switchTab(tabName) {
    state.activeTab = tabName;
    const currentState = vscode.getState() || {};
    vscode.setState({
      ...currentState,
      messages: state.messages,
      chatId: state.chatId,
      activeTab: tabName,
    });

    // Update tab buttons
    tabBtns.forEach(btn => {
      btn.classList.toggle('active', btn.dataset.tab === tabName);
    });

    // Update tab content
    document.querySelectorAll('.tab-content').forEach(el => {
      el.classList.toggle('active', el.id === 'tab-' + tabName);
    });

    // Load MCP data when switching to MCP tab
    if (tabName === 'mcp' && state.mcpServers.length === 0 && !state.mcpLoading) {
      loadMcpServers();
    }

    // Load project analysis when switching to Project tab
    if (tabName === 'project') {
      loadProjectAnalysis();
    }
  }

  // ── Tab Event Listeners ───────────────────────────────────────────────────

  tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      switchTab(btn.dataset.tab);
    });
  });

  // ── MCP: Load Servers ─────────────────────────────────────────────────────

  async function loadMcpServers() {
    if (state.mcpLoading) return;
    state.mcpLoading = true;
    showMcpLoading(true);

    try {
      const servers = await call('mcp/listServers', {});
      // Ensure servers is always an array (defensive against API returning an object)
      state.mcpServers = Array.isArray(servers) ? servers : [];
      state.mcpServerTools = {};
      state.mcpExpandedServer = null;
      renderMcpServers();
    } catch (err) {
      showError(`Failed to load MCP servers: ${err.message}`);
      state.mcpServers = [];
      renderMcpServers();
    } finally {
      state.mcpLoading = false;
      showMcpLoading(false);
    }
  }

  // ── MCP: Load Tools for a Server ──────────────────────────────────────────

  async function loadServerTools(serverName) {
    // If already loaded, just toggle expand
    if (state.mcpServerTools[serverName]) {
      toggleMcpExpand(serverName);
      return;
    }

    try {
      const tools = await call('mcp/listServerTools', { serverName });
      // Defensive: ensure tools is an array before storing it.
      // The Rust core should always return an array, but if something goes wrong
      // (e.g. an error object is returned), we fall back to an empty array
      // to avoid "tools.forEach is not a function" crashes in renderMcpServers.
      state.mcpServerTools[serverName] = Array.isArray(tools) ? tools : [];
      state.mcpExpandedServer = serverName;
      renderMcpServers();
    } catch (err) {
      showError(`Failed to load tools for ${serverName}: ${err.message}`);
    }
  }

  function toggleMcpExpand(serverName) {
    if (state.mcpExpandedServer === serverName) {
      state.mcpExpandedServer = null;
    } else {
      state.mcpExpandedServer = serverName;
    }
    renderMcpServers();
  }

  // ── MCP: Render ───────────────────────────────────────────────────────────

  function showMcpLoading(visible) {
    if (visible) {
      const spinner = document.createElement('div');
      spinner.className = 'loading-spinner';
      spinner.id = 'mcp-loading';
      spinner.innerHTML = '<div class="spinner"></div><span>Loading MCP servers...</span>';
      mcpServerList.innerHTML = '';
      mcpServerList.appendChild(spinner);
    } else {
      const spinner = document.getElementById('mcp-loading');
      if (spinner) spinner.remove();
    }
  }

  function renderMcpServers() {
    mcpServerList.innerHTML = '';

    if (state.mcpServers.length === 0) {
      mcpEmptyState.classList.remove('hidden');
      return;
    }

    mcpEmptyState.classList.add('hidden');

    state.mcpServers.forEach(server => {
      const card = document.createElement('div');
      card.className = 'mcp-server-card';

      // ── Header (clickable) ──
      const header = document.createElement('div');
      header.className = 'mcp-server-header';

      // Status dot
      const statusDot = document.createElement('span');
      const statusClass = getStatusClass(server);
      statusDot.className = 'mcp-server-status-dot ' + statusClass;
      header.appendChild(statusDot);

      // Name
      const name = document.createElement('span');
      name.className = 'mcp-server-name';
      name.textContent = server.name;
      header.appendChild(name);

      // Type badge
      const typeBadge = document.createElement('span');
      typeBadge.className = 'mcp-server-type';
      typeBadge.textContent = server.server_type || 'embedded';
      header.appendChild(typeBadge);

      // Tool count
      const toolCount = document.createElement('span');
      toolCount.className = 'mcp-server-tool-count';
      toolCount.textContent = server.tool_count + ' tool' + (server.tool_count !== 1 ? 's' : '');
      header.appendChild(toolCount);

      // Expand icon
      const expandIcon = document.createElement('span');
      expandIcon.className = 'mcp-server-expand-icon' +
        (state.mcpExpandedServer === server.name ? ' expanded' : '');
      expandIcon.textContent = '▶';
      header.appendChild(expandIcon);

      header.addEventListener('click', () => {
        loadServerTools(server.name);
      });

      header.appendChild(document.createElement('div'));

      card.appendChild(header);

      // ── Description ──
      if (server.description) {
        const desc = document.createElement('div');
        desc.className = 'mcp-server-description';
        desc.textContent = server.description;
        card.appendChild(desc);
      }

      // ── Tool list (expanded) ──
      if (state.mcpExpandedServer === server.name) {
        const tools = state.mcpServerTools[server.name];
        const toolList = document.createElement('div');
        toolList.className = 'mcp-tool-list';

        if (!tools || tools.length === 0) {
          const empty = document.createElement('div');
          empty.className = 'mcp-tool-item';
          empty.style.color = 'var(--text-muted)';
          empty.style.fontSize = '11px';
          empty.style.padding = '12px 12px 12px 24px';
          empty.textContent = 'No tools available';
          toolList.appendChild(empty);
        } else {
          tools.forEach(tool => {
            const item = document.createElement('div');
            item.className = 'mcp-tool-item';

            // Icon
            const icon = document.createElement('span');
            icon.className = 'mcp-tool-icon';
            icon.textContent = '⚡';
            item.appendChild(icon);

            // Info
            const info = document.createElement('div');
            info.className = 'mcp-tool-info';

            const toolName = document.createElement('div');
            toolName.className = 'mcp-tool-name';
            toolName.textContent = tool.name;
            info.appendChild(toolName);

            if (tool.description) {
              const desc = document.createElement('div');
              desc.className = 'mcp-tool-description';
              desc.textContent = tool.description;
              info.appendChild(desc);
            }

            // Input schema (descriptive parameter list)
            if (tool.input_schema && tool.input_schema.properties && Object.keys(tool.input_schema.properties).length > 0) {
              const params = document.createElement('div');
              params.className = 'mcp-tool-params';

              const required = Array.isArray(tool.input_schema.required) ? tool.input_schema.required : [];

              Object.entries(tool.input_schema.properties).forEach(([name, prop]) => {
                const param = document.createElement('div');
                param.className = 'mcp-tool-param';

                const isRequired = required.includes(name);
                const type = prop.type || 'any';
                const desc = prop.description || '';

                const reqClass = isRequired ? 'required' : 'optional';
                param.innerHTML = '<span class="mcp-tool-param-name">' + name + '</span> <span class="mcp-tool-param-type">' + type + '</span><span class="mcp-tool-param-required ' + reqClass + '">' + (isRequired ? 'required' : 'optional') + '</span>' + (desc ? '<span class="mcp-tool-param-desc"> — ' + desc + '</span>' : '');

                params.appendChild(param);
              });

              info.appendChild(params);
            }

            item.appendChild(info);

            // Enabled badge
            const enabled = document.createElement('span');
            enabled.className = 'mcp-tool-enabled ' + (tool.enabled !== false ? 'yes' : 'no');
            enabled.textContent = tool.enabled !== false ? 'enabled' : 'disabled';
            item.appendChild(enabled);

            toolList.appendChild(item);
          });
        }

        card.appendChild(toolList);
      }

      mcpServerList.appendChild(card);
    });
  }

  function getStatusClass(server) {
    // Try to derive status from properties or default to online
    const status = server.properties && server.properties.status;
    if (status === 'error') return 'error';
    if (status === 'connecting') return 'connecting';
    if (status === 'offline') return 'offline';
    return 'online';
  }

  // ── MCP: Refresh ──────────────────────────────────────────────────────────

  mcpRefreshBtn.addEventListener('click', () => {
    state.mcpServers = [];
    state.mcpServerTools = {};
    state.mcpExpandedServer = null;
    loadMcpServers();
  });

  // ── Chat Settings Panel ─────────────────────────────────────────────────────

  const settingsBtn = document.getElementById('settings-btn');
  const settingsPanel = document.getElementById('chat-settings-panel');
  const settingsClose = document.getElementById('chat-settings-close');
  const configApiKey = document.getElementById('config-api-key');
  const configModel = document.getElementById('config-model');
  const configApiUrl = document.getElementById('config-api-url');
  const configSaveBtn = document.getElementById('config-save-btn');
  const configStatus = document.getElementById('config-status');
  const configToggleKey = document.getElementById('config-toggle-key');

  /**
   * Load DeepSeek configuration from the graph-backed config store.
   */
  async function loadConfig() {
    try {
      const result = await call('config/getAll', {});

      // Defensive: handle missing or error result
      if (!result || result.error) {
        throw new Error(result?.error || 'No response from config backend');
      }

      const config = result.config || {};

      // Populate fields from stored values (or keep defaults)
      if (config['deepseek.api_key'] !== null && config['deepseek.api_key'] !== undefined) {
        configApiKey.value = config['deepseek.api_key'];
      }
      if (config['deepseek.model'] !== null && config['deepseek.model'] !== undefined) {
        configModel.value = config['deepseek.model'];
      }
      if (config['deepseek.api_url'] !== null && config['deepseek.api_url'] !== undefined) {
        configApiUrl.value = config['deepseek.api_url'];
      }

      showConfigStatus('Configuration loaded', 'success');
    } catch (err) {
      showConfigStatus(`Failed to load config: ${err.message}`, 'error');
    }
  }

  /**
   * Save DeepSeek configuration to the graph-backed config store.
   */
  async function saveConfig() {
    const apiKey = configApiKey.value.trim();
    const model = configModel.value;
    const apiUrl = configApiUrl.value;

    if (!apiKey) {
      showConfigStatus('API key is required', 'error');
      return;
    }

    configSaveBtn.disabled = true;
    configSaveBtn.textContent = 'Saving...';

    try {
      // Save each field individually via config/set
      await call('config/set', { key: 'deepseek.api_key', value: apiKey });
      await call('config/set', { key: 'deepseek.model', value: model });
      await call('config/set', { key: 'deepseek.api_url', value: apiUrl });

      showConfigStatus('Configuration saved successfully!', 'success');
    } catch (err) {
      showConfigStatus(`Failed to save config: ${err.message}`, 'error');
    } finally {
      configSaveBtn.disabled = false;
      configSaveBtn.textContent = 'Save Configuration';
    }
  }

  function showConfigStatus(message, type) {
    configStatus.textContent = message;
    configStatus.className = 'config-status config-status-' + type;
    // Auto-hide success messages after 3 seconds
    if (type === 'success') {
      setTimeout(() => {
        configStatus.className = 'config-status';
      }, 3000);
    }
  }

  // Toggle API key visibility
  configToggleKey.addEventListener('click', () => {
    const isPassword = configApiKey.type === 'password';
    configApiKey.type = isPassword ? 'text' : 'password';
    configToggleKey.textContent = isPassword ? '🙈' : '👁';
  });

  // Save button
  configSaveBtn.addEventListener('click', saveConfig);

  // ── Settings Panel: Open / Close ──────────────────────────────────────────

  function openSettings() {
    settingsPanel.classList.remove('hidden');
    loadConfig();
  }

  function closeSettings() {
    settingsPanel.classList.add('hidden');
  }

  settingsBtn.addEventListener('click', openSettings);
  settingsClose.addEventListener('click', closeSettings);

  // Close settings panel when clicking outside the panel body
  settingsPanel.addEventListener('click', (e) => {
    if (e.target === settingsPanel) {
      closeSettings();
    }
  });

  // ── Markdown Renderer ────────────────────────────────────────────────────

  /**
   * Convert a Markdown string to safe HTML.
   * Handles the subset of Markdown commonly emitted by LLMs.
   *
   * This function processes markdown line-by-line to avoid regex state issues
   * and ensure consistent rendering across multiple calls.
   */
  function markdownToHtml(md) {
    if (!md) return '';

    // Normalize non-string content to a string before calling .replace()
    if (typeof md !== 'string') {
      if (Array.isArray(md)) {
        md = md.map(function(part) {
          return typeof part === 'object' && part !== null
            ? (part.text || part.content || JSON.stringify(part))
            : String(part);
        }).join('\n');
      } else if (typeof md === 'object' && md !== null) {
        md = md.text || md.content || JSON.stringify(md);
      } else {
        md = String(md);
      }
    }

    // Escape HTML entities first to prevent XSS
    var html = md
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');

    // Tables (| ... | ... |) — must process before inline code and line-by-line
    // Use a marker to protect tables from further transformations
    var tableBlocks = [];
    var tableBlockIndex = 0;
    html = html.replace(/((?:^\|.+\|\s*\n)+)/gm, function(tableBlock) {
      var lines = tableBlock.trim().split('\n');
      if (lines.length < 2) return tableBlock; // need at least header + separator

      // Check second line is a separator (|----|----|)
      var sepLine = lines[1].trim();
      if (!/^\|[-:| ]+\|$/.test(sepLine)) return tableBlock;

      // Parse column count from separator
      var cols = sepLine.split('|').filter(function(s) { return s.trim() !== ''; }).length;
      if (cols === 0) return tableBlock;

      // Parse alignment from separator
      var alignments = [];
      var sepParts = sepLine.split('|').filter(function(s) { return s.trim() !== ''; });
      for (var a = 0; a < sepParts.length; a++) {
        var part = sepParts[a].trim();
        if (part.startsWith(':') && part.endsWith(':')) {
          alignments.push('center');
        } else if (part.endsWith(':')) {
          alignments.push('right');
        } else if (part.startsWith(':')) {
          alignments.push('left');
        } else {
          alignments.push(null);
        }
      }

      var html = '<table>\n';

      // Header row (first line)
      var headerCells = lines[0].split('|').filter(function(s) { return s.trim() !== ''; });
      html += '<thead><tr>';
      for (var h = 0; h < headerCells.length; h++) {
        var alignAttr = alignments[h] ? ' style="text-align:' + alignments[h] + '"' : '';
        html += '<th' + alignAttr + '>' + headerCells[h].trim() + '</th>';
      }
      html += '</tr></thead>\n';

      // Body rows (third line onwards)
      html += '<tbody>';
      for (var r = 2; r < lines.length; r++) {
        var cells = lines[r].split('|').filter(function(s) { return s.trim() !== ''; });
        if (cells.length === 0) continue;
        html += '<tr>';
        for (var c = 0; c < cells.length; c++) {
          var alignAttr2 = alignments[c] ? ' style="text-align:' + alignments[c] + '"' : '';
          html += '<td' + alignAttr2 + '>' + cells[c].trim() + '</td>';
        }
        html += '</tr>';
      }
      html += '</tbody>\n';
      html += '</table>';

      var marker = '%%%TABLEBLOCK' + (tableBlockIndex++) + '%%%';
      tableBlocks.push(html);
      return marker;
    });

    // Fenced code blocks (```lang ... ```) — must process before inline code
    // Use a marker to protect code blocks from further transformations
    var codeBlocks = [];
    var codeBlockIndex = 0;
    html = html.replace(/```(\w*)\n?([\s\S]*?)```/g, function(_, lang, code) {
      var langAttr = lang ? ' class="language-' + lang + '"' : '';
      var block = '<pre><code' + langAttr + '>' + code.trim() + '</code></pre>';
      var marker = '%%%CODEBLOCK' + (codeBlockIndex++) + '%%%';
      codeBlocks.push(block);
      return marker;
    });

    // Inline code (`code`) — protect from bold/italic transformations
    var inlineCodes = [];
    var inlineCodeIndex = 0;
    html = html.replace(/`([^`]+)`/g, function(_, code) {
      var marker = '%%%INLINECODE' + (inlineCodeIndex++) + '%%%';
      inlineCodes.push('<code>' + code + '</code>');
      return marker;
    });

    // Process line-by-line for block-level elements
    var lines = html.split('\n');
    var result = [];
    var inList = null; // 'ul', 'ol', or null
    var inBlockquote = false;

    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];

      // Horizontal rules
      if (/^([-*_]){3,}\s*$/.test(line)) {
        closeList(result, inList);
        inList = null;
        result.push('<hr>');
        continue;
      }

      // Headers
      var headerMatch = line.match(/^(#{1,6})\s+(.+)$/);
      if (headerMatch) {
        closeList(result, inList);
        inList = null;
        var level = headerMatch[1].length;
        result.push('<h' + level + '>' + headerMatch[2] + '</h' + level + '>');
        continue;
      }

      // Blockquotes (> text, which becomes > text after HTML escaping)
      var bqMatch = line.match(/^&gt;\s+(.+)$/);
      if (bqMatch) {
        if (!inBlockquote) {
          closeList(result, inList);
          inList = null;
          result.push('<blockquote>');
          inBlockquote = true;
        }
        result.push(bqMatch[1] + '<br>');
        continue;
      } else if (inBlockquote) {
        result.push('</blockquote>');
        inBlockquote = false;
      }

      // Unordered list items
      var ulMatch = line.match(/^[\s]*[-*]\s+(.+)$/);
      if (ulMatch) {
        if (inList !== 'ul') {
          closeList(result, inList);
          inList = 'ul';
          result.push('<ul>');
        }
        result.push('<li>' + ulMatch[1] + '</li>');
        continue;
      }

      // Ordered list items
      var olMatch = line.match(/^\s*\d+\.\s+(.+)$/);
      if (olMatch) {
        if (inList !== 'ol') {
          closeList(result, inList);
          inList = 'ol';
          result.push('<ol>');
        }
        result.push('<li>' + olMatch[1] + '</li>');
        continue;
      }

      // Non-list, non-header line — close any open list
      if (inList) {
        closeList(result, inList);
        inList = null;
      }

      // Empty line — paragraph separator (collapse consecutive empties)
      if (line.trim() === '') {
        // Only add a separator if the last item isn't already empty
        if (result.length === 0 || result[result.length - 1] !== '') {
          result.push('');
        }
        continue;
      }

      // Regular text line — will be wrapped in <p> later
      result.push(line);
    }

    // Close any remaining open tags
    closeList(result, inList);
    if (inBlockquote) {
      result.push('</blockquote>');
    }

    // Join lines and wrap paragraphs
    html = result.join('\n');

    // Wrap consecutive non-empty, non-tag lines in <p> tags
    // Filter out empty blocks from collapsed blank lines
    var paragraphs = html.split('\n\n').filter(function(b) { return b.trim() !== ''; });
    html = paragraphs.map(function(block) {
      var trimmed = block.trim();
      // Don't wrap if already a block-level element or a table/code marker
      if (/^<(h[1-6]|ul|ol|li|pre|blockquote|hr|p|table)/.test(trimmed)) {
        return trimmed;
      }
      if (/^%%%(TABLEBLOCK|CODEBLOCK)/.test(trimmed)) {
        return trimmed;
      }
      return '<p>' + trimmed.replace(/\n/g, '<br>') + '</p>';
    }).join('\n');


    // Restore inline code markers
    for (var j = 0; j < inlineCodes.length; j++) {
      html = html.replace('%%%INLINECODE' + j + '%%%', inlineCodes[j]);
    }

    // Apply inline formatting (bold, italic, links) — but NOT inside code blocks
    // We process the text outside of code block markers
    var parts = html.split(/(%%%CODEBLOCK\d+%%%)/);
    for (var k = 0; k < parts.length; k++) {
      if (parts[k].indexOf('%%%CODEBLOCK') === 0) {
        continue; // Skip code block markers
      }
      // Bold (**text**)
      parts[k] = parts[k].replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
      // Italic (*text*)
      parts[k] = parts[k].replace(/\*([^*]+)\*/g, '<em>$1</em>');
      // Links ([text](url))
      parts[k] = parts[k].replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');
    }
    html = parts.join('');

    // Restore table block markers
    for (var j = 0; j < tableBlocks.length; j++) {
      html = html.replace('%%%TABLEBLOCK' + j + '%%%', tableBlocks[j]);
    }

    // Restore code block markers
    for (var j = 0; j < codeBlocks.length; j++) {
      html = html.replace('%%%CODEBLOCK' + j + '%%%', codeBlocks[j]);
    }

    return html;

  }

  /** Helper: close an open list tag */
  function closeList(result, listType) {
    if (listType === 'ul') {
      result.push('</ul>');
    } else if (listType === 'ol') {
      result.push('</ol>');
    }
  }

  // ── Message Rendering ─────────────────────────────────────────────────────

  function renderMessages() {
    // Clear existing messages (keep empty state)
    const existingMessages = messagesEl.querySelectorAll('.message-widget');
    existingMessages.forEach(el => el.remove());

    if (state.messages.length === 0) {
      emptyState.classList.remove('hidden');
      return;
    }

    emptyState.classList.add('hidden');

    state.messages.forEach(msg => {
      const el = createMessageElement(msg);
      messagesEl.appendChild(el);
    });

    scrollToBottom();
  }

  /**
   * Create a DOM element for any message.
   *
   * Every message is rendered through the widget dispatch system.
   * If the message has no explicit widget, a synthetic 'chat-message'
   * widget is created from the message's role/content/intent/timestamp.
   *
   * This ensures a single rendering path for all message types.
   */
  function createMessageElement(msg) {
    // Normalize content to a string if it's an object/array
    var contentStr = msg.content;
    if (contentStr !== null && contentStr !== undefined && typeof contentStr !== 'string') {
      if (Array.isArray(contentStr)) {
        contentStr = contentStr.map(function(part) {
          return typeof part === 'object' && part !== null
            ? (part.text || part.content || JSON.stringify(part))
            : String(part);
        }).join('\n');
      } else if (typeof contentStr === 'object') {
        contentStr = contentStr.text || contentStr.content || JSON.stringify(contentStr);
      } else {
        contentStr = String(contentStr);
      }
    }

    // Normalize: every message gets a widget, even plain chat
    const widget = msg.widget || {
      widgetId: msg.id,
      widgetType: 'chat-message',
      state: {
        role: msg.role,
        content: contentStr,
        intent: msg.intent,
        timestamp: msg.timestamp
      }
    };

    const container = document.createElement('div');
    container.className = 'message-widget';
    container.dataset.widgetId = widget.widgetId;
    container.dataset.widgetType = widget.widgetType;

    // Render Intent Badge if present at the top of the container
    if (msg.role === 'assistant' && msg.intent) {
      const intentBadge = document.createElement('div');
      intentBadge.className = 'message-intent-badge';
      const intent = msg.intent;
      const name = intent.name || 'unknown';
      const route = intent.route || '';
      const confidence = intent.confidence || 0;
      const confidencePct = Math.round(confidence * 100);
      intentBadge.textContent = '🎯 ' + name + ' (' + route + ', ' + confidencePct + '%)';
      container.appendChild(intentBadge);
    }

    // Skip rendering content if it's a plain chat-message and is empty
    if (widget.widgetType === 'chat-message' && !widget.state.content) {
      // Don't render empty chat bubbles
    } else {
      container.appendChild(renderWidgetContent(widget.widgetType, widget.state));
    }
    return container;
  }

  // ── Widget Rendering ──────────────────────────────────────────────────────

  /**
   * Create a widget DOM element based on widget type and state.
   * The element gets data-widget-id and data-widget-type attributes
   * so it can be found and updated in-place later.
   */
  function createWidgetElement(widget) {
    const container = document.createElement('div');
    container.className = 'message-widget';
    container.dataset.widgetId = widget.widgetId;
    container.dataset.widgetType = widget.widgetType;

    const content = renderWidgetContent(widget.widgetType, widget.state);
    container.appendChild(content);

    return container;
  }

  /**
   * Dispatch to the correct widget renderer based on type.
   */
  function renderWidgetContent(widgetType, state) {
    switch (widgetType) {
      case 'chat-message':
        return renderChatMessage(state);
      case 'build-list':
        return renderBuildList(state);
      case 'radio-group':
        return renderRadioGroup(state);
      case 'checkbox-list':
        return renderCheckboxList(state);
      case 'progress-bar':
        return renderProgressBar(state);
      default:
        const fallback = document.createElement('div');
        fallback.className = 'widget-unknown';
        fallback.textContent = 'Unknown widget type: ' + widgetType;
        return fallback;
    }
  }

  // ── Chat Message Widget ───────────────────────────────────────────────────

  /**
   * Render a normal chat message bubble (user, assistant, system, or error).
   * This is the default widget type used when no explicit widget is attached.
   */
  function renderChatMessage(state) {
    if (state.role === 'system') {
      const div = document.createElement('div');
      div.className = 'message-system';
      div.textContent = state.content;
      return div;
    }

    if (state.role === 'error') {
      const div = document.createElement('div');
      div.className = 'message-error';
      div.textContent = state.content;
      return div;
    }

    const div = document.createElement('div');
    div.className = 'message message-' + (state.role === 'user' ? 'user' : 'assistant');

    const role = document.createElement('div');
    role.className = 'message-role';
    role.textContent = state.role === 'user' ? 'You' : 'Spire';
    div.appendChild(role);

    if (state.content) {
      const content = document.createElement('div');
      content.className = 'message-content';
      // Render Markdown for assistant messages, plain text for user messages
      if (state.role === 'assistant') {
        content.innerHTML = markdownToHtml(state.content);
      } else {
        content.textContent = state.content;
      }
      div.appendChild(content);
    }

    if (state.timestamp) {
      const ts = document.createElement('div');
      ts.className = 'message-timestamp';
      ts.textContent = formatTime(state.timestamp);
      div.appendChild(ts);
    }

    return div;
  }

  // ── Build List Widget ─────────────────────────────────────────────────────

  function renderBuildList(state) {
    const container = document.createElement('div');
    container.className = 'widget-build-list';

    // Optional title
    if (state.title) {
      const title = document.createElement('div');
      title.className = 'widget-title';
      title.textContent = state.title;
      container.appendChild(title);
    }

    const builds = state.builds || [];
    builds.forEach(function(build) {
      const row = document.createElement('div');
      row.className = 'build-row build-' + (build.status || 'pending');

      // Status icon
      const icon = document.createElement('span');
      icon.className = 'build-icon';
      icon.textContent = statusIcon(build.status);
      row.appendChild(icon);

      // Name
      const name = document.createElement('span');
      name.className = 'build-name';
      name.textContent = build.name;
      row.appendChild(name);

      // Build system type badge
      if (build.type) {
        const typeBadge = document.createElement('span');
        typeBadge.className = 'build-type-badge';
        typeBadge.textContent = build.type;
        row.appendChild(typeBadge);
      }

      // Duration
      const duration = document.createElement('span');
      duration.className = 'build-duration';
      if (build.duration_ms !== null && build.duration_ms !== undefined) {
        duration.textContent = formatDuration(build.duration_ms);
      } else if (build.status === 'running') {
        duration.textContent = '...';
      } else {
        duration.textContent = '—';
      }
      row.appendChild(duration);

      // Status label
      const label = document.createElement('span');
      label.className = 'build-status-label';
      label.textContent = build.status;
      row.appendChild(label);

      // Click to expand/collapse build log
      if (build.log) {
        const logSection = document.createElement('div');
        logSection.className = 'build-log';
        logSection.textContent = build.log;

        row.addEventListener('click', function() {
          logSection.classList.toggle('expanded');
        });

        container.appendChild(row);
        container.appendChild(logSection);
      } else {
        container.appendChild(row);
      }
    });

    return container;
  }

  /** Map build status to a unicode icon */
  function statusIcon(status) {
    switch (status) {
      case 'success': return '✓';
      case 'running': return '⏳';
      case 'error':   return '✗';
      case 'skipped': return '–';
      default:        return '○';  // pending
    }
  }

  /** Format milliseconds to human-readable */
  function formatDuration(ms) {
    if (ms < 1000) return ms + 'ms';
    if (ms < 60000) return (ms / 1000).toFixed(1) + 's';
    return (ms / 60000).toFixed(1) + 'm';
  }

  // ── Radio Group Widget ────────────────────────────────────────────────────

  function renderRadioGroup(state) {
    const container = document.createElement('div');
    container.className = 'widget-radio-group';

    if (state.label) {
      const label = document.createElement('div');
      label.className = 'widget-label';
      label.textContent = state.label;
      container.appendChild(label);
    }

    const options = state.options || [];
    options.forEach(function(opt) {
      const optionRow = document.createElement('div');
      optionRow.className = 'widget-option';
      if (opt.disabled) {
        optionRow.classList.add('disabled');
      }
      if (state.selected === opt.value) {
        optionRow.classList.add('selected');
      }

      const radio = document.createElement('span');
      radio.className = 'widget-radio';
      radio.textContent = state.selected === opt.value ? '●' : '○';
      optionRow.appendChild(radio);

      const label = document.createElement('span');
      label.className = 'widget-option-label';
      label.textContent = opt.label;
      optionRow.appendChild(label);

      if (!opt.disabled) {
        optionRow.addEventListener('click', function() {
          handleWidgetInteraction(container.closest('[data-widget-id]').dataset.widgetId, opt.value);
        });
      }

      container.appendChild(optionRow);
    });

    return container;
  }

  // ── Checkbox List Widget ──────────────────────────────────────────────────

  function renderCheckboxList(state) {
    const container = document.createElement('div');
    container.className = 'widget-checkbox-list';

    if (state.label) {
      const label = document.createElement('div');
      label.className = 'widget-label';
      label.textContent = state.label;
      container.appendChild(label);
    }

    const options = state.options || [];
    const selected = state.selected || [];
    options.forEach(function(opt) {
      const optionRow = document.createElement('div');
      optionRow.className = 'widget-option';
      if (selected.indexOf(opt.value) !== -1) {
        optionRow.classList.add('selected');
      }

      const checkbox = document.createElement('span');
      checkbox.className = 'widget-checkbox';
      checkbox.textContent = selected.indexOf(opt.value) !== -1 ? '☑' : '☐';
      optionRow.appendChild(checkbox);

      const label = document.createElement('span');
      label.className = 'widget-option-label';
      label.textContent = opt.label;
      optionRow.appendChild(label);

      optionRow.addEventListener('click', function() {
        handleWidgetInteraction(container.closest('[data-widget-id]').dataset.widgetId, opt.value);
      });

      container.appendChild(optionRow);
    });

    return container;
  }

  // ── Progress Bar Widget ───────────────────────────────────────────────────

  function renderProgressBar(state) {
    const container = document.createElement('div');
    container.className = 'widget-progress-bar';

    if (state.label) {
      const label = document.createElement('div');
      label.className = 'widget-label';
      label.textContent = state.label;
      container.appendChild(label);
    }

    const track = document.createElement('div');
    track.className = 'progress-track';

    const fill = document.createElement('div');
    fill.className = 'progress-fill';
    var value = typeof state.value === 'number' ? state.value : 0;
    fill.style.width = Math.min(100, Math.max(0, value)) + '%';

    if (state.status === 'error') {
      fill.classList.add('progress-error');
    } else if (state.status === 'success') {
      fill.classList.add('progress-success');
    }

    track.appendChild(fill);
    container.appendChild(track);

    const pct = document.createElement('div');
    pct.className = 'progress-percent';
    pct.textContent = value + '%';
    container.appendChild(pct);

    return container;
  }

  // ── Widget Interaction ────────────────────────────────────────────────────

  /**
   * Called when a user interacts with a widget (clicks a radio option, checkbox, etc.).
   * Sends a JSON-RPC call to the extension host which forwards to the backend.
   */
  function handleWidgetInteraction(widgetId, value) {
    if (!state.connected) return;
    call('widget/interact', { widgetId, value }).catch(function(err) {
      showError('Widget interaction failed: ' + err.message);
    });
  }

  /**
   * Update a widget's state in-place (called when event/widget/update arrives).
   * Finds the widget DOM element by data-widget-id and re-renders its content.
   */
  function updateWidgetInPlace(widgetId, newState) {
    const el = document.querySelector('[data-widget-id="' + widgetId + '"]');
    if (!el) return;

    const widgetType = el.dataset.widgetType;
    // Clear and re-render just the content
    el.innerHTML = '';
    el.appendChild(renderWidgetContent(widgetType, newState));

    // Also update the message in state so it persists across reloads
    for (var i = 0; i < state.messages.length; i++) {
      var msg = state.messages[i];
      if (msg.widget && msg.widget.widgetId === widgetId) {
        msg.widget.state = newState;
        break;
      }
    }
  }

  function addMessage(msg) {
    state.messages.push(msg);
    const currentState = vscode.getState() || {};
    vscode.setState({ ...currentState, messages: state.messages, chatId: state.chatId, activeTab: state.activeTab });

    emptyState.classList.add('hidden');
    const el = createMessageElement(msg);
    messagesEl.appendChild(el);
    scrollToBottom();
  }

  function scrollToBottom() {
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function formatTime(isoString) {
    const date = new Date(isoString);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  // ── Connection Status ─────────────────────────────────────────────────────

  function setConnected(connected) {
    state.connected = connected;
    statusDot.className = 'status-dot ' + (connected ? 'connected' : 'disconnected');
    statusText.textContent = connected ? 'Connected' : 'Disconnected';
    sendBtn.disabled = !connected || state.isProcessing;
    inputEl.disabled = !connected;
  }

  function setConnecting() {
    statusDot.className = 'status-dot connecting';
    statusText.textContent = 'Connecting...';
    sendBtn.disabled = true;
    inputEl.disabled = true;
  }

  function showError(message) {
    errorBanner.textContent = message;
    errorBanner.classList.add('visible');
    setTimeout(() => {
      errorBanner.classList.remove('visible');
    }, 5000);
  }

  function showTyping(visible) {
    if (visible) {
      typingIndicator.classList.remove('hidden');
    } else {
      typingIndicator.classList.add('hidden');
    }
  }

  // ── Build Progress (shown as italic system messages in chat) ──────────────

  /**
   * Show or update a build progress message in the chat area.
   * The message appears as an italicized system message.
   * When the build completes, the message updates to "build complete".
   */
  function showBuildProgress(message) {
    // Determine if this is a completion/final message
    const isComplete = /complete|done|finished|success/i.test(message);

    // If we already have a build progress message element, update it
    if (state.buildProgressMessageId) {
      const existingEl = document.getElementById(state.buildProgressMessageId);
      if (existingEl) {
        existingEl.textContent = message;
        if (isComplete) {
          existingEl.classList.remove('build-progress');
          existingEl.classList.add('build-complete');
        }
        return;
      }
    }

    // Create a new build progress message
    const id = 'build-progress-' + Date.now();
    state.buildProgressMessageId = id;

    const div = document.createElement('div');
    div.id = id;
    div.className = isComplete ? 'message-system build-complete' : 'message-system build-progress';
    div.textContent = message;

    // Insert before the typing indicator if it's visible, otherwise append
    if (!typingIndicator.classList.contains('hidden')) {
      messagesEl.insertBefore(div, typingIndicator);
    } else {
      messagesEl.appendChild(div);
    }

    scrollToBottom();
  }

  /**
   * Clear the build progress message from the chat area.
   */
  function clearBuildProgress() {
    if (state.buildProgressMessageId) {
      const el = document.getElementById(state.buildProgressMessageId);
      if (el) el.remove();
      state.buildProgressMessageId = null;
    }
  }

  // ── Chat Actions ──────────────────────────────────────────────────────────

  async function sendMessage() {
    const text = inputEl.value.trim();
    if (!text || state.isProcessing || !state.connected) return;

    inputEl.value = '';
    inputEl.style.height = 'auto';
    sendBtn.disabled = true;
    state.isProcessing = true;

    // Add user message immediately
    addMessage({
      role: 'user',
      content: text,
      timestamp: new Date().toISOString(),
    });

    showTyping(true);

    try {
      // Send user message to environment server via extension host
      await call('chat/append', {
        chatId: state.chatId,
        content: text,
        options: { role: 'user' },
      });

      // Send the full conversation to DeepSeek via llm/complete
      // Include the full messages array so the LLM has conversation context
      const conversationMessages = state.messages.map(msg => ({
        id: msg.id || `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        role: msg.role,
        content: msg.content,
        timestamp: msg.timestamp || new Date().toISOString(),
      }));
      const llmResult = await call('llm/complete', { messages: conversationMessages });

      // Check for error responses from the LLM (e.g. missing API key)
      if (llmResult?.error) {
        throw new Error(llmResult.error);
      }

      const reply = llmResult?.content || '';

      // Extract intent info from the response (if present)
      const intentInfo = llmResult?.intent || null;

      // Store the assistant reply
      await call('chat/append', {
        chatId: state.chatId,
        content: reply,
        options: { role: 'assistant' },
      });

      // Display the assistant reply in the UI
      addMessage({
        role: 'assistant',
        content: reply,
        timestamp: new Date().toISOString(),
        intent: intentInfo,
      });
    } catch (err) {
      showError(`Failed to send message: ${err.message}`);
      addMessage({
        role: 'error',
        content: `Error: ${err.message}`,
      });
    } finally {
      showTyping(false);
      state.isProcessing = false;
      sendBtn.disabled = !state.connected;
    }
  }

  async function clearChat() {
    try {
      await call('chat/clear', { chatId: state.chatId });
      state.messages = [];
      const currentState = vscode.getState() || {};
      vscode.setState({ ...currentState, messages: [], chatId: state.chatId, activeTab: state.activeTab });
      renderMessages();
    } catch (err) {
      showError(`Failed to clear chat: ${err.message}`);
    }
  }

  async function newChat() {
    try {
      // Create a new chat with a unique ID
      const newId = 'chat-' + Date.now();
      await call('chat/append', {
        chatId: newId,
        content: 'New conversation started',
        options: { role: 'system' },
      });
      state.chatId = newId;
      state.messages = [];
      const currentState = vscode.getState() || {};
      vscode.setState({ ...currentState, messages: [], chatId: newId, activeTab: state.activeTab });
      renderMessages();
    } catch (err) {
      showError(`Failed to create new chat: ${err.message}`);
    }
  }

  async function loadChat() {
    try {
      const chat = await call('chat/getActive', {});
      if (chat && chat.messages && chat.messages.length > 0) {
        // Only overwrite local state if the subprocess has actual messages.
        // This preserves messages restored from vscode.getState() when the
        // subprocess was restarted (e.g. on window reload) and lost its state.
        state.messages = chat.messages;
        state.chatId = chat.id;
        const currentState = vscode.getState() || {};
        vscode.setState({ ...currentState, messages: state.messages, chatId: state.chatId, activeTab: state.activeTab });
        renderMessages();
      } else if (state.messages.length === 0) {
        // No messages anywhere — show empty state
        renderMessages();
      }
      // If we have local messages but subprocess is empty, keep local messages
    } catch (err) {
      showError(`Failed to load chat: ${err.message}`);
    }
  }

  // ── Event Listeners ───────────────────────────────────────────────────────

  // Send on Enter (Shift+Enter for newline)
  inputEl.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  });

  // Auto-resize textarea
  inputEl.addEventListener('input', () => {
    inputEl.style.height = 'auto';
    inputEl.style.height = Math.min(inputEl.scrollHeight, 120) + 'px';
    sendBtn.disabled = !inputEl.value.trim() || !state.connected || state.isProcessing;
  });

  sendBtn.addEventListener('click', sendMessage);

  // Chat header buttons — bound to the statically-defined toolbar in index.html
  document.getElementById('clear-btn').addEventListener('click', clearChat);
  document.getElementById('new-chat-btn').addEventListener('click', newChat);

  // ── Project Tab (graph + overview) ────────────────────────────────────────

  const projectContent = document.getElementById('project-content');
  const projectEmptyState = document.getElementById('project-empty-state');
  const projectRefreshBtn = document.getElementById('project-refresh-btn');
  const projectOverviewBtn = document.getElementById('project-overview-btn');
  const projectGraphBtn = document.getElementById('project-graph-btn');
  const projectGraph = document.getElementById('project-graph');

  /**
   * Load project analysis data from the core subprocess.
   */
  async function loadProjectAnalysis() {
    // Show loading state
    projectContent.innerHTML = '';
    const loading = document.createElement('div');
    loading.className = 'empty-state';
    loading.innerHTML = '<div class="empty-state-icon">📊</div><div class="empty-state-text">Loading...</div><div class="empty-state-hint">Fetching project analysis...</div>';
    projectContent.appendChild(loading);

    try {
      const analysis = await call('project/analysis', {});
      renderProjectAnalysis(analysis);
    } catch (err) {
      projectContent.innerHTML = '';
      const error = document.createElement('div');
      error.className = 'empty-state';
      error.innerHTML = '<div class="empty-state-icon">⚠️</div><div class="empty-state-text">Failed to load</div><div class="empty-state-hint">' + err.message + '</div>';
      projectContent.appendChild(error);
    }
  }

  /**
   * Render the project analysis data as a read-only view.
   */
  function renderProjectAnalysis(analysis) {
    projectContent.innerHTML = '';

    if (!analysis) {
      projectContent.appendChild(projectEmptyState);
      return;
    }

    // ── Overview section ──
    if (analysis.overview) {
      const overview = document.createElement('div');
      overview.className = 'project-section';

      const title = document.createElement('div');
      title.className = 'project-section-title';
      title.textContent = 'Overview';
      overview.appendChild(title);

      const table = document.createElement('div');
      table.className = 'project-info-table';

      addInfoRow(table, 'Project', analysis.overview.name || '-');
      addInfoRow(table, 'Root', analysis.overview.root || '-');
      addInfoRow(table, 'Language', analysis.overview.language || '-');
      addInfoRow(table, 'Build System', analysis.overview.build_system || '-');
      addInfoRow(table, 'Files', String(analysis.overview.file_count ?? '-'));
      addInfoRow(table, 'Lines of Code', String(analysis.overview.loc ?? '-'));

      overview.appendChild(table);
      projectContent.appendChild(overview);
    }

    // ── Dependencies section ──
    if (analysis.dependencies && analysis.dependencies.length > 0) {
      const deps = document.createElement('div');
      deps.className = 'project-section';

      const title = document.createElement('div');
      title.className = 'project-section-title';
      title.textContent = 'Dependencies (' + analysis.dependencies.length + ')';
      deps.appendChild(title);

      const list = document.createElement('div');
      list.className = 'project-dependency-list';

      analysis.dependencies.forEach(function(dep) {
        const item = document.createElement('div');
        item.className = 'project-dependency-item';
        item.textContent = dep.name + (dep.version ? ' ' + dep.version : '');
        list.appendChild(item);
      });

      deps.appendChild(list);
      projectContent.appendChild(deps);
    }

    // ── Modules / Packages section ──
    if (analysis.modules && analysis.modules.length > 0) {
      const mods = document.createElement('div');
      mods.className = 'project-section';

      const title = document.createElement('div');
      title.className = 'project-section-title';
      title.textContent = 'Modules (' + analysis.modules.length + ')';
      mods.appendChild(title);

      const list = document.createElement('div');
      list.className = 'project-module-list';

      analysis.modules.forEach(function(mod) {
        const item = document.createElement('div');
        item.className = 'project-module-item';
        item.textContent = mod.name || mod.path || '-';
        list.appendChild(item);
      });

      mods.appendChild(list);
      projectContent.appendChild(mods);
    }

    // ── Build targets section ──
    if (analysis.build_targets && analysis.build_targets.length > 0) {
      const targets = document.createElement('div');
      targets.className = 'project-section';

      const title = document.createElement('div');
      title.className = 'project-section-title';
      title.textContent = 'Build Targets (' + analysis.build_targets.length + ')';
      targets.appendChild(title);

      const list = document.createElement('div');
      list.className = 'project-target-list';

      analysis.build_targets.forEach(function(target) {
        const item = document.createElement('div');
        item.className = 'project-target-item';
        item.textContent = target.name + (target.kind ? ' (' + target.kind + ')' : '');
        list.appendChild(item);
      });

      targets.appendChild(list);
      projectContent.appendChild(targets);
    }

    // ── Fallback if no sections rendered ──
    if (projectContent.children.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'empty-state';
      empty.innerHTML = '<div class="empty-state-icon">📊</div><div class="empty-state-text">No analysis data</div><div class="empty-state-hint">Project analysis returned no data</div>';
      projectContent.appendChild(empty);
    }
  }

  /**
   * Helper: add a key-value row to an info table.
   */
  function addInfoRow(table, label, value) {
    const row = document.createElement('div');
    row.className = 'project-info-row';

    const labelEl = document.createElement('span');
    labelEl.className = 'project-info-label';
    labelEl.textContent = label;

    const valueEl = document.createElement('span');
    valueEl.className = 'project-info-value';
    valueEl.textContent = value;

    row.appendChild(labelEl);
    row.appendChild(valueEl);
    table.appendChild(row);
  }

  // Project refresh button
  projectRefreshBtn.addEventListener('click', function() {
    loadProjectAnalysis();
  });

  // View toggle: Overview ↔ Graph
  projectOverviewBtn.addEventListener('click', function() {
    projectContent.classList.remove('hidden');
    projectGraph.classList.add('hidden');
    projectOverviewBtn.classList.add('active-view');
    projectGraphBtn.classList.remove('active-view');
  });

  projectGraphBtn.addEventListener('click', function() {
    projectContent.classList.add('hidden');
    projectGraph.classList.remove('hidden');
    projectOverviewBtn.classList.remove('active-view');
    projectGraphBtn.classList.add('active-view');
    renderProjectGraph();
  });

  /**
   * Render the project tree as a Cytoscape.js interactive graph.
   * Transforms the analysis data (ProjectFileTree) into nodes and edges.
   */
  function renderProjectGraph() {
    const cyEl = document.getElementById('cy');
    if (!cyEl) return;

    // Clear any existing graph
    cyEl.innerHTML = '';

    // Collect nodes and edges from the last loaded analysis data
    const cy = cytoscape({
      container: cyEl,
      style: [
        {
          selector: 'node',
          style: {
            'background-color': '#4a90d9',
            'label': 'data(label)',
            'font-size': '10px',
            'text-valign': 'bottom',
            'color': '#ccc',
            'width': 'mapData(fileCount, 0, 500, 30, 80)',
            'height': 'mapData(fileCount, 0, 500, 30, 80)',
          }
        },
        {
          selector: 'node[role = "source_code"]',
          style: { 'background-color': '#4a90d9' }
        },
        {
          selector: 'node[role = "tests"]',
          style: { 'background-color': '#50b86c' }
        },
        {
          selector: 'node[role = "documentation"]',
          style: { 'background-color': '#e6a817' }
        },
        {
          selector: 'node[role = "build_config"]',
          style: { 'background-color': '#9b59b6', shape: 'diamond' }
        },
        {
          selector: 'node[role = "config"]',
          style: { 'background-color': '#e74c3c', shape: 'square' }
        },
        {
          selector: 'edge',
          style: {
            'width': 1,
            'line-color': '#555',
            'target-arrow-color': '#555',
            'target-arrow-shape': 'triangle',
            'arrow-scale': 0.6,
            'curve-style': 'bezier'
          }
        },
        {
          selector: 'edge[label = "builds"]',
          style: { 'line-color': '#9b59b6', 'target-arrow-color': '#9b59b6' }
        },
        {
          selector: 'edge[label = "packages"]',
          style: { 'line-color': '#2ecc71', 'target-arrow-color': '#2ecc71', 'width': 2 }
        }
      ],
      layout: { name: 'dagre', rankDir: 'TB', padding: 20 }
    });

    // Build elements from the project structure
    const elements = [];

    // Add project root
    elements.push({
      group: 'nodes',
      data: { id: 'root', label: 'Project Root', role: 'root', fileCount: 0 }
    });

    // Add build systems as nodes
    // Build metadata might have build_types, config_files, etc.
    // Add languages
    // Add directories from the analysis

    // For now, load from the analysis data that was already fetched
    // We store it in the closure scope
    if (window._lastAnalysis) {
      const analysis = window._lastAnalysis;
      addDirectoryNodes(elements, analysis.root || analysis.overview, 'root');
    }

    cy.add(elements);
    cy.layout({ name: 'dagre', rankDir: 'TB', padding: 20 }).run();

    // Enable node selection tooltip
    cy.on('tap', 'node', function(evt) {
      const node = evt.target;
      const data = node.data();
      const tooltip = document.createElement('div');
      tooltip.className = 'cy-tooltip';
      tooltip.innerHTML = '<strong>' + (data.label || data.id) + '</strong>'
        + (data.path ? '<br>Path: ' + data.path : '')
        + (data.language ? '<br>Language: ' + data.language : '')
        + (data.fileCount ? '<br>Files: ' + data.fileCount : '')
        + (data.lines ? '<br>Lines: ' + data.lines : '');
      tooltip.style.cssText = 'position:fixed;background:#333;color:#fff;padding:6px 10px;border-radius:4px;font-size:11px;z-index:1000;pointer-events:none;max-width:300px';
      document.body.appendChild(tooltip);
      const pos = evt.originalEvent;
      tooltip.style.left = (pos.clientX + 12) + 'px';
      tooltip.style.top = (pos.clientY + 12) + 'px';
      setTimeout(() => tooltip.remove(), 3000);
    });
  }

  /**
   * Recursively add directory nodes from the analysis data.
   */
  function addDirectoryNodes(elements, node, parentId) {
    if (!node || !node.name) return;

    const id = node.path || node.name;
    const role = node.role || 'directory';
    const fileCount = node.total_file_count || node.file_count || 0;
    const lines = node.total_lines || node.lines || 0;

    elements.push({
      group: 'nodes',
      data: {
        id: id,
        label: node.name,
        role: role,
        path: node.path || '',
        fileCount: fileCount,
        lines: lines,
        language: node.language || ''
      }
    });

    elements.push({
      group: 'edges',
      data: { id: parentId + '-' + id, source: parentId, target: id }
    });

    // Add subdirectories
    if (node.directories) {
      node.directories.forEach(function(dir) {
        addDirectoryNodes(elements, dir, id);
      });
    }

    // Add files summary as a synthetic node
    if (node.files && node.files.length > 0) {
      const filesId = id + '-files';
      elements.push({
        group: 'nodes',
        data: {
          id: filesId,
          label: node.files.length + ' files',
          role: 'other',
          fileCount: node.files.length
        }
      });
      elements.push({
        group: 'edges',
        data: { id: id + '-files-edge', source: id, target: filesId }
      });
    }
  }

  // Store last analysis data for graph rendering (overlays the text render)
  const _origRenderProject = renderProjectAnalysis;
  renderProjectAnalysis = function(analysis) {
    window._lastAnalysis = analysis;
    _origRenderProject(analysis);
  };

  // ── Tools Tab (real-time tool usage feed) ─────────────────────────────────

  const toolsFeed = document.getElementById('tools-feed');
  const toolsEmptyState = document.getElementById('tools-empty-state');
  const toolsClearBtn = document.getElementById('tools-clear-btn');

  /**
   * Render the tool events feed.
   */
  function renderToolEvents() {
    toolsFeed.innerHTML = '';

    if (state.toolEvents.length === 0) {
      toolsEmptyState.classList.remove('hidden');
      return;
    }

    toolsEmptyState.classList.add('hidden');

    state.toolEvents.forEach(event => {
      const card = document.createElement('div');
      card.className = 'tool-event-card tool-event-' + event.status;

      // ── Header row ──
      const header = document.createElement('div');
      header.className = 'tool-event-header';

      // Status icon
      const statusIcon = document.createElement('span');
      statusIcon.className = 'tool-event-status-icon';
      if (event.status === 'running') {
        statusIcon.innerHTML = '<span class="tool-spinner"></span>';
      } else if (event.status === 'success') {
        statusIcon.textContent = '✓';
      } else {
        statusIcon.textContent = '✗';
      }
      header.appendChild(statusIcon);

      // Tool name
      const name = document.createElement('span');
      name.className = 'tool-event-name';
      name.textContent = event.tool_name;
      header.appendChild(name);

      // Duration badge
      if (event.duration_ms !== null) {
        const duration = document.createElement('span');
        duration.className = 'tool-event-duration';
        const ms = event.duration_ms;
        if (ms < 1000) {
          duration.textContent = ms + 'ms';
        } else {
          duration.textContent = (ms / 1000).toFixed(1) + 's';
        }
        header.appendChild(duration);
      }

      // Timestamp
      const ts = document.createElement('span');
      ts.className = 'tool-event-timestamp';
      ts.textContent = formatTime(event.timestamp);
      header.appendChild(ts);

      card.appendChild(header);

      // ── Args (collapsible) ──
      if (event.args && typeof event.args === 'object' && Object.keys(event.args).length > 0) {
        const argsSection = document.createElement('div');
        argsSection.className = 'tool-event-detail';
        argsSection.textContent = 'Args: ' + JSON.stringify(event.args, null, 1);
        card.appendChild(argsSection);
      }

      // ── Result (for success) ──
      if (event.status === 'success' && event.result) {
        const resultSection = document.createElement('div');
        resultSection.className = 'tool-event-detail tool-event-result';
        const resultText = typeof event.result === 'string'
          ? event.result.substring(0, 500)
          : JSON.stringify(event.result).substring(0, 500);
        resultSection.textContent = 'Result: ' + resultText;
        if (resultText.length >= 500) {
          resultSection.textContent += '...';
        }
        card.appendChild(resultSection);
      }

      // ── Error (for error) ──
      if (event.status === 'error' && event.error) {
        const errorSection = document.createElement('div');
        errorSection.className = 'tool-event-detail tool-event-error-detail';
        errorSection.textContent = 'Error: ' + (typeof event.error === 'string' ? event.error : JSON.stringify(event.error));
        card.appendChild(errorSection);
      }

      toolsFeed.appendChild(card);
    });
  }

  // Tools clear button
  toolsClearBtn.addEventListener('click', () => {
    state.toolEvents = [];
    renderToolEvents();
  });

  // ── Handle Messages from Extension Host ───────────────────────────────────

  window.addEventListener('message', (event) => {
    const msg = event.data;

    switch (msg.type) {
      case 'status':
        if (msg.connected) {
          setConnected(true);
          loadChat();
        } else {
          setConnected(false);
        }
        break;

      case 'connecting':
        setConnecting();
        break;

      case 'response':
        // Resolve pending request
        const pending = state.pendingRequests.get(msg.id);
        if (pending) {
          state.pendingRequests.delete(msg.id);
          if (msg.error) {
            pending.reject(new Error(msg.error));
          } else {
            pending.resolve(msg.result);
          }
        }
        break;

      case 'notification':
        // Handle server-pushed notifications
        if (msg.method === 'event/chat/message') {
          const params = msg.params || {};
          // Support both { message: { role, content, widget } } and { content, role, widget } formats
          const message = params.message || params;
          if (message && message.content) {
            showTyping(false);
            state.isProcessing = false;
            sendBtn.disabled = !state.connected;
            addMessage({
              role: message.role || 'assistant',
              content: message.content,
              timestamp: message.timestamp || new Date().toISOString(),
              intent: message.intent || null,
              widget: message.widget || null,
            });
          }
        } else if (msg.method === 'event/widget/update') {
          // Update a widget's state in-place (e.g. build-list gets updated with real results)
          const params = msg.params || {};
          const widgetId = params.widgetId;
          const newState = params.state;
          if (widgetId && newState) {
            updateWidgetInPlace(widgetId, newState);
          }
        } else if (msg.method === 'event/system/progress') {

          const params = msg.params || {};
          const message = params.message;

          if (message) {
            // Show build progress as an italic system message in the chat area
            showBuildProgress(message);
          }
        } else if (msg.method === 'event/tool/start') {
          // Tool execution started
          const p = msg.params || {};
          const eventId = ++state.toolEventIdCounter;
          const toolEvent = {
            id: eventId,
            tool_name: p.tool_name || 'unknown',
            status: 'running',
            args: p.args,
            tool_call_id: p.tool_call_id,
            timestamp: p.timestamp || new Date().toISOString(),
            duration_ms: null,
            result: null,
            error: null,
          };
          state.toolEvents.unshift(toolEvent);
          // Trim to max
          if (state.toolEvents.length > state.maxToolEvents) {
            state.toolEvents.length = state.maxToolEvents;
          }
          renderToolEvents();
        } else if (msg.method === 'event/tool/result') {
          // Tool execution completed successfully
          const p = msg.params || {};
          const tool_call_id = p.tool_call_id;
          // Find the matching running event by tool_call_id
          const existing = state.toolEvents.find(e => e.tool_call_id === tool_call_id && e.status === 'running');
          if (existing) {
            existing.status = 'success';
            existing.duration_ms = p.duration_ms;
            existing.result = p.result;
            renderToolEvents();
          }
        } else if (msg.method === 'event/tool/error') {
          // Tool execution failed
          const p = msg.params || {};
          const tool_call_id = p.tool_call_id;
          const existing = state.toolEvents.find(e => e.tool_call_id === tool_call_id && e.status === 'running');
          if (existing) {
            existing.status = 'error';
            existing.duration_ms = p.duration_ms;
            existing.error = p.error;
            renderToolEvents();
          }
        }
        break;


      case 'error':
        showError(msg.message);
        showTyping(false);
        state.isProcessing = false;
        sendBtn.disabled = !state.connected;
        break;

      case 'ready':
        // Extension host is ready — request connection status
        setConnecting();
        break;

    }
  });

  // ── Initialize ────────────────────────────────────────────────────────────

  // Restore messages from previous state
  if (state.messages.length > 0) {
    renderMessages();
  }

  // Restore active tab
  switchTab(state.activeTab);

  // Signal that the webview is ready
  vscode.postMessage({ type: 'webviewReady' });

  // Focus input
  inputEl.focus();

  console.log('Spire Webview initialized');
})();