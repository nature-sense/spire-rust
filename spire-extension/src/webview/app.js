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
  };

  // Restore previous state if available
  const previousState = vscode.getState();
  if (previousState) {
    state.messages = previousState.messages || [];
    state.chatId = previousState.chatId || 'default';
    state.activeTab = previousState.activeTab || 'chat';
  }

  // ── DOM References ────────────────────────────────────────────────────────

  const messagesEl = document.getElementById('messages');
  const emptyState = document.getElementById('empty-state');
  const inputEl = document.getElementById('message-input');
  const sendBtn = document.getElementById('send-btn');
  const statusDot = document.getElementById('status-dot');
  const statusText = document.getElementById('status-text');
  const errorBanner = document.getElementById('error-banner');
  const typingIndicator = document.getElementById('typing-indicator');

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

      // Timeout after 30s
      setTimeout(() => {
        if (state.pendingRequests.has(id)) {
          state.pendingRequests.delete(id);
          reject(new Error(`Request timed out: ${method}`));
        }
      }, 30000);
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
    vscode.setState({
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
      state.mcpServerTools[serverName] = tools || [];
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
                param.innerHTML = `<span class="mcp-tool-param-name">${name}</span> <span class="mcp-tool-param-type">${type}</span><span class="mcp-tool-param-required ${reqClass}">${isRequired ? 'required' : 'optional'}</span>${desc ? '<span class="mcp-tool-param-desc"> — ' + desc + '</span>' : ''}`;

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

  // ── Config Tab ─────────────────────────────────────────────────────────────

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

  // Load config when switching to the config tab
  const originalSwitchTab = switchTab;
  switchTab = function(tabName) {
    originalSwitchTab(tabName);
    if (tabName === 'config') {
      loadConfig();
    }
  };

  // ── Message Rendering ─────────────────────────────────────────────────────

  function renderMessages() {
    // Clear existing messages (keep empty state)
    const existingMessages = messagesEl.querySelectorAll('.message, .message-system, .message-error');
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

  function createMessageElement(msg) {
    const div = document.createElement('div');

    if (msg.role === 'system') {
      div.className = 'message-system';
      div.textContent = msg.content;
    } else if (msg.role === 'error') {
      div.className = 'message-error';
      div.textContent = msg.content;
    } else {
      div.className = `message message-${msg.role === 'user' ? 'user' : 'assistant'}`;

      const role = document.createElement('div');
      role.className = 'message-role';
      role.textContent = msg.role === 'user' ? 'You' : 'Spire';
      div.appendChild(role);

      const content = document.createElement('div');
      content.className = 'message-content';
      content.textContent = msg.content;
      div.appendChild(content);

      if (msg.timestamp) {
        const ts = document.createElement('div');
        ts.className = 'message-timestamp';
        ts.textContent = formatTime(msg.timestamp);
        div.appendChild(ts);
      }
    }

    return div;
  }

  function addMessage(msg) {
    state.messages.push(msg);
    vscode.setState({ messages: state.messages, chatId: state.chatId, activeTab: state.activeTab });

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
      // Send to environment server via extension host
      await call('chat/append', {
        chatId: state.chatId,
        content: text,
        options: { role: 'user' },
      });

      // The assistant reply will come as a notification
      // (handled in the message listener below)
    } catch (err) {
      showError(`Failed to send message: ${err.message}`);
      addMessage({
        role: 'error',
        content: `Error: ${err.message}`,
      });
      showTyping(false);
      state.isProcessing = false;
      sendBtn.disabled = !state.connected;
    }
  }

  async function clearChat() {
    try {
      await call('chat/clear', { chatId: state.chatId });
      state.messages = [];
      vscode.setState({ messages: [], chatId: state.chatId, activeTab: state.activeTab });
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
      vscode.setState({ messages: [], chatId: newId, activeTab: state.activeTab });
      renderMessages();
    } catch (err) {
      showError(`Failed to create new chat: ${err.message}`);
    }
  }

  async function loadChat() {
    try {
      const chat = await call('chat/getActive', {});
      if (chat && chat.messages) {
        state.messages = chat.messages;
        state.chatId = chat.id;
        vscode.setState({ messages: state.messages, chatId: state.chatId, activeTab: state.activeTab });
        renderMessages();
      }
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

  // Chat header buttons (moved into tab-toolbar for chat)
  // We add them dynamically since the header was replaced by the tab bar
  const chatToolbar = document.createElement('div');
  chatToolbar.className = 'tab-toolbar';
  chatToolbar.innerHTML = `
    <span class="tab-toolbar-title">Chat</span>
    <div style="display:flex;gap:4px">
      <button class="header-btn" id="clear-btn" title="Clear conversation">🗑 Clear</button>
      <button class="header-btn" id="new-chat-btn" title="New chat">✚ New</button>
    </div>
  `;
  const tabChat = document.getElementById('tab-chat');
  tabChat.insertBefore(chatToolbar, tabChat.firstChild);

  // Re-bind chat buttons
  document.getElementById('clear-btn').addEventListener('click', clearChat);
  document.getElementById('new-chat-btn').addEventListener('click', newChat);

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
          const message = msg.params?.message;
          if (message) {
            showTyping(false);
            state.isProcessing = false;
            sendBtn.disabled = !state.connected;
            addMessage(message);
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
