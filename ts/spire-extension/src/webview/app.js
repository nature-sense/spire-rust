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

  // Startup overlay references
  const startupOverlay = document.getElementById('startup-overlay');
  const startupStatus = document.getElementById('startup-status');

  // Restore previous state if available (persists across webview reloads)
  const previousState = vscode.getState();
  if (previousState && previousState.messages && previousState.messages.length > 0) {
    state.messages = previousState.messages;
    state.chatId = previousState.chatId || 'default';
    state.activeTab = previousState.activeTab || 'chat';
  }

  // NOTE: The startup overlay is NOT hidden based on persisted state here.
  // Instead, the extension host sends an 'initStatus' message when the webview
  // signals it's ready. If the core has already completed initialization,
  // the overlay is hidden immediately. If not, it stays visible until the
  // progress notification with percent=100 arrives.
  // This avoids the bug where persisted state from a previous VS Code session
  // would incorrectly hide the overlay during a fresh initialization.


  /**
   * Safety fallback: if the Rust core never sends percent=100 (e.g. due to a
   * bug or edge case), dismiss the overlay after 120 seconds so the user isn't
   * stuck looking at it forever. This timer is cleared once completeStartup()
   * is called via the normal progress notification path.
   */
  let startupFallbackTimer = setTimeout(() => {
    completeStartup();
  }, 120000);

  function completeStartup() {
    // Clear the safety fallback timer if it hasn't fired yet
    if (startupFallbackTimer) {
      clearTimeout(startupFallbackTimer);
      startupFallbackTimer = null;
    }

    startupStatus.textContent = 'Starting Spire — complete!';

    // Persist that startup has completed so subsequent webview loads
    // (e.g. when re-selecting the sidebar) skip the startup overlay.
    const currentState = vscode.getState() || {};
    vscode.setState({ ...currentState, startupComplete: true });

    // Hide the overlay after a brief delay
    setTimeout(() => {
      startupOverlay.classList.add('hidden');
    }, 800);
  }


  // Tab references
  const tabBar = document.getElementById('tab-bar');
  const tabBtns = tabBar.querySelectorAll('.tab-btn');

  // MCP references
  const mcpServerList = document.getElementById('mcp-server-list');
  const mcpEmptyState = document.getElementById('mcp-empty-state');
  const mcpRefreshBtn = document.getElementById('mcp-refresh-btn');
  const mcpImportBtn = document.getElementById('mcp-import-btn');
  const mcpAddBtn = document.getElementById('mcp-add-btn');

  // MCP Config Modal references
  const mcpConfigModal = document.getElementById('mcp-config-modal');
  const mcpConfigModalTitle = document.getElementById('mcp-config-modal-title');
  const mcpConfigModalClose = document.getElementById('mcp-config-modal-close');
  const mcpConfigName = document.getElementById('mcp-config-name');
  const mcpConfigTransport = document.getElementById('mcp-config-transport');
  const mcpConfigCommand = document.getElementById('mcp-config-command');
  const mcpConfigArgs = document.getElementById('mcp-config-args');
  const mcpConfigUrl = document.getElementById('mcp-config-url');
  const mcpConfigHeaders = document.getElementById('mcp-config-headers');
  const mcpConfigEnv = document.getElementById('mcp-config-env');
  const mcpConfigAutostart = document.getElementById('mcp-config-autostart');
  const mcpConfigSaveBtn = document.getElementById('mcp-config-save-btn');
  const mcpConfigCancelBtn = document.getElementById('mcp-config-cancel-btn');
  const mcpConfigDeleteBtn = document.getElementById('mcp-config-delete-btn');
  const mcpConfigStatus = document.getElementById('mcp-config-status');
  const mcpConfigCommandGroup = document.getElementById('mcp-config-command-group');
  const mcpConfigArgsGroup = document.getElementById('mcp-config-args-group');
  const mcpConfigUrlGroup = document.getElementById('mcp-config-url-group');
  const mcpConfigHeadersGroup = document.getElementById('mcp-config-headers-group');

  // State for the MCP config editor
  let mcpConfigEditingName = null; // null = creating new, string = editing existing

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

      // ── Action buttons (edit / delete) ──
      const actions = document.createElement('div');
      actions.className = 'mcp-server-actions';

      const editBtn = document.createElement('button');
      editBtn.className = 'mcp-server-edit-btn';
      editBtn.title = 'Edit MCP server configuration';
      editBtn.textContent = '✎';
      editBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        showMcpConfigModal(server.name);
      });
      actions.appendChild(editBtn);

      const deleteBtn = document.createElement('button');
      deleteBtn.className = 'mcp-server-delete-btn';
      deleteBtn.title = 'Delete MCP server configuration';
      deleteBtn.textContent = '✕';
      deleteBtn.addEventListener('click', async (e) => {
        e.stopPropagation();
        if (!confirm(`Delete MCP server "${server.name}"?`)) return;
        try {
          await call('mcp/config/delete', { name: server.name });
          state.mcpServers = [];
          state.mcpServerTools = {};
          state.mcpExpandedServer = null;
          loadMcpServers();
        } catch (err) {
          showError(`Failed to delete ${server.name}: ${err.message}`);
        }
      });
      actions.appendChild(deleteBtn);

      header.appendChild(actions);

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

  // ── MCP: Import Config ────────────────────────────────────────────────────

  mcpImportBtn.addEventListener('click', async () => {
    // Send a message to the extension host to open a file dialog
    vscode.postMessage({ type: 'mcpImportConfig' });
  });

  // ── MCP Config Editor Modal ────────────────────────────────────────────────

  /**
   * Show the MCP config editor modal.
   * @param {string|null} serverName - null for creating new, string for editing existing
   */
  function showMcpConfigModal(serverName) {
    mcpConfigEditingName = serverName;
    mcpConfigStatus.textContent = '';
    mcpConfigStatus.className = 'config-status';

    if (serverName) {
      // Editing existing — find the server config from the stored config
      mcpConfigModalTitle.textContent = 'Edit MCP Server';
      mcpConfigDeleteBtn.classList.remove('hidden');
      loadMcpConfigForEdit(serverName);
    } else {
      // Creating new
      mcpConfigModalTitle.textContent = 'Add MCP Server';
      mcpConfigDeleteBtn.classList.add('hidden');
      resetMcpConfigForm();
    }

    mcpConfigModal.classList.remove('hidden');
  }

  function hideMcpConfigModal() {
    mcpConfigModal.classList.add('hidden');
    mcpConfigEditingName = null;
  }

  function resetMcpConfigForm() {
    mcpConfigName.value = '';
    mcpConfigTransport.value = 'stdio';
    mcpConfigCommand.value = '';
    mcpConfigArgs.value = '';
    mcpConfigUrl.value = '';
    mcpConfigHeaders.value = '';
    mcpConfigEnv.value = '';
    mcpConfigAutostart.checked = true;
    updateTransportVisibility();
  }

  function updateTransportVisibility() {
    const isStdio = mcpConfigTransport.value === 'stdio';
    mcpConfigCommandGroup.style.display = isStdio ? '' : 'none';
    mcpConfigArgsGroup.style.display = isStdio ? '' : 'none';
    mcpConfigUrlGroup.style.display = isStdio ? 'none' : '';
    mcpConfigHeadersGroup.style.display = isStdio ? 'none' : '';
  }

  mcpConfigTransport.addEventListener('change', updateTransportVisibility);

  /**
   * Load an existing server's config from the backend and populate the form.
   */
  async function loadMcpConfigForEdit(serverName) {
    try {
      const result = await call('mcp/config/get', {});
      const servers = result.servers || [];
      const server = servers.find(s => s.name === serverName);
      if (!server) {
        showMcpConfigStatus('Server not found in config', 'error');
        return;
      }

      mcpConfigName.value = server.name || '';
      mcpConfigCommand.value = server.command || '';
      mcpConfigArgs.value = (server.args || []).join('\n');
      mcpConfigUrl.value = server.url || '';
      mcpConfigHeaders.value = server.headers ? JSON.stringify(server.headers, null, 2) : '';
      mcpConfigEnv.value = server.env ? JSON.stringify(server.env, null, 2) : '';
      mcpConfigAutostart.checked = server.autostart !== false;

      // Determine transport type
      if (server.url) {
        mcpConfigTransport.value = 'http';
      } else {
        mcpConfigTransport.value = 'stdio';
      }
      updateTransportVisibility();
    } catch (err) {
      showMcpConfigStatus(`Failed to load config: ${err.message}`, 'error');
    }
  }

  function showMcpConfigStatus(message, type) {
    mcpConfigStatus.textContent = message;
    mcpConfigStatus.className = 'config-status config-status-' + type;
    if (type === 'success') {
      setTimeout(() => {
        mcpConfigStatus.className = 'config-status';
      }, 3000);
    }
  }

  /**
   * Save the MCP config from the form (create or update).
   */
  async function saveMcpConfig() {
    const name = mcpConfigName.value.trim();
    if (!name) {
      showMcpConfigStatus('Name is required', 'error');
      return;
    }

    const isStdio = mcpConfigTransport.value === 'stdio';
    const params = { name };

    if (isStdio) {
      const command = mcpConfigCommand.value.trim();
      if (!command) {
        showMcpConfigStatus('Command is required for stdio transport', 'error');
        return;
      }
      params.command = command;
      const argsLines = mcpConfigArgs.value.trim();
      if (argsLines) {
        params.args = argsLines.split('\n').map(s => s.trim()).filter(s => s.length > 0);
      }
    } else {
      const url = mcpConfigUrl.value.trim();
      if (!url) {
        showMcpConfigStatus('URL is required for HTTP transport', 'error');
        return;
      }
      params.url = url;
      const headersStr = mcpConfigHeaders.value.trim();
      if (headersStr) {
        try {
          params.headers = JSON.parse(headersStr);
        } catch (e) {
          showMcpConfigStatus('Headers must be valid JSON', 'error');
          return;
        }
      }
    }

    const envStr = mcpConfigEnv.value.trim();
    if (envStr) {
      try {
        params.env = JSON.parse(envStr);
      } catch (e) {
        showMcpConfigStatus('Environment variables must be valid JSON', 'error');
        return;
      }
    }

    params.autostart = mcpConfigAutostart.checked;

    mcpConfigSaveBtn.disabled = true;
    mcpConfigSaveBtn.textContent = 'Saving...';

    try {
      await call('mcp/config/save', params);
      showMcpConfigStatus('Configuration saved successfully!', 'success');
      // Refresh the server list
      state.mcpServers = [];
      state.mcpServerTools = {};
      state.mcpExpandedServer = null;
      loadMcpServers();
      // Close modal after a brief delay
      setTimeout(() => {
        hideMcpConfigModal();
      }, 800);
    } catch (err) {
      showMcpConfigStatus(`Failed to save: ${err.message}`, 'error');
    } finally {
      mcpConfigSaveBtn.disabled = false;
      mcpConfigSaveBtn.textContent = 'Save';
    }
  }

  /**
   * Delete the currently-edited MCP server config.
   */
  async function deleteMcpConfig() {
    if (!mcpConfigEditingName) {
      console.error('deleteMcpConfig: mcpConfigEditingName is null');
      return;
    }

    const name = mcpConfigName.value.trim() || mcpConfigEditingName;
    if (!confirm(`Delete MCP server "${name}"?`)) return;

    mcpConfigDeleteBtn.disabled = true;
    mcpConfigDeleteBtn.textContent = 'Deleting...';

    try {
      const result = await call('mcp/config/delete', { name });
      // Check for error in the result (the Rust core may return { error: "..." }
      // which the extension host wraps as a JSON-RPC error, but just in case)
      if (result && result.error) {
        throw new Error(result.error);
      }
      showMcpConfigStatus('Deleted successfully!', 'success');
      // Refresh the server list
      state.mcpServers = [];
      state.mcpServerTools = {};
      state.mcpExpandedServer = null;
      loadMcpServers();
      setTimeout(() => {
        hideMcpConfigModal();
      }, 800);
    } catch (err) {
      console.error('deleteMcpConfig failed:', err);
      showMcpConfigStatus(`Failed to delete: ${err.message}`, 'error');
    } finally {
      mcpConfigDeleteBtn.disabled = false;
      mcpConfigDeleteBtn.textContent = '🗑 Delete';
    }
  }

  // ── MCP Config Modal Event Listeners ──────────────────────────────────────

  mcpAddBtn.addEventListener('click', () => {
    showMcpConfigModal(null);
  });

  mcpConfigModalClose.addEventListener('click', hideMcpConfigModal);
  mcpConfigCancelBtn.addEventListener('click', hideMcpConfigModal);

  // Close modal on overlay click
  mcpConfigModal.addEventListener('click', (e) => {
    if (e.target === mcpConfigModal) {
      hideMcpConfigModal();
    }
  });

  mcpConfigSaveBtn.addEventListener('click', saveMcpConfig);
  mcpConfigDeleteBtn.addEventListener('click', deleteMcpConfig);

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
      // Render Markdown for assistant messages, plain text for user messages
      if (msg.role === 'assistant') {
        content.innerHTML = markdownToHtml(msg.content);
      } else {
        content.textContent = msg.content;
      }
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

      // Send the prompt to DeepSeek via llm/complete
      const llmResult = await call('llm/complete', { prompt: text });
      const reply = llmResult?.content || '';

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
          // Do NOT dismiss the startup overlay here — the overlay is dismissed
          // only when the Rust core sends event/system/progress with percent=100,
          // which happens after the full initialization sequence completes.
          // Dismissing on TCP connection alone would hide the overlay before
          // the embedder download, graph init, project sync, etc. have finished.
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
        } else if (msg.method === 'event/system/progress') {
          // Handle startup progress — hide overlay when complete
          const params = msg.params || {};
          const percent = params.percent;
          const message = params.message;

          if (message) {
            startupStatus.textContent = message;
          }

          // When we hit 100%, complete the startup
          if (percent === 100) {
            completeStartup();
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
        } else if (msg.method === 'event/mcp/config/imported') {
          // MCP config was imported — refresh the server list
          state.mcpServers = [];
          state.mcpServerTools = {};
          state.mcpExpandedServer = null;
          loadMcpServers();
        }
        break;


      case 'error':
        showError(msg.message);
        showTyping(false);
        state.isProcessing = false;
        sendBtn.disabled = !state.connected;
        break;

      case 'initStatus':
        // The extension host tells us whether the core has finished initializing.
        // If complete, hide the overlay immediately (e.g. sidebar re-activation
        // in the same VS Code session where init already finished).
        if (msg.complete) {
          completeStartup();
        }
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
