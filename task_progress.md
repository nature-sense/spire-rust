# Implementation Plan: Store MCP Config in Graph Database

## Steps

- [ ] 1. Add `LoadConfigFromGraph` message to McpClientActor + `load_config_from_entries()` to McpClientManager
- [ ] 2. Update SystemActor to send graph configs to MCP client after bootstrap
- [ ] 3. Update CoordinatorActor to reload MCP client after import
- [ ] 4. Add import button to MCP tab in index.html + sidebar-provider.ts
- [ ] 5. Add file dialog handler in app.js
- [ ] 6. Update graph-schema.md documentation
