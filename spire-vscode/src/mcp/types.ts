export interface McpRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: any;
}

export interface McpResponse {
  jsonrpc: '2.0';
  id: number;
  result?: any;
  error?: {
    code: number;
    message: string;
    data?: any;
  };
}

export interface McpNotification {
  jsonrpc: '2.0';
  method: string;
  params?: any;
}

export type McpMessage = McpResponse | McpNotification;

export interface ToolCallParams {
  name: string;
  arguments: Record<string, any>;
}
