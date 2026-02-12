import { apiClient, ApiError } from './client';
import type { McpServerListResponse, ToggleMcpServerResponse, UpdateApiKeyResponse } from './types';

/**
 * Get all MCP servers with their status.
 * Returns empty list if MCP is not configured (503/404).
 */
export async function getMcpServers(): Promise<McpServerListResponse> {
  try {
    return await apiClient
      .get('api/mcp/servers', { retry: 0 })
      .json<McpServerListResponse>();
  } catch (err) {
    if (err instanceof ApiError && (err.status === 503 || err.status === 404)) {
      return { servers: [], total: 0, enabled_count: 0 };
    }
    throw err;
  }
}

/**
 * Toggle a MCP server's enable/disable state.
 */
export async function toggleMcpServer(
  name: string,
  enable: boolean
): Promise<ToggleMcpServerResponse> {
  return await apiClient
    .post(`api/mcp/servers/${encodeURIComponent(name)}/toggle`, {
      json: { enable },
      timeout: 30000,
    })
    .json<ToggleMcpServerResponse>();
}

/**
 * Update API key for a MCP server.
 */
export async function updateMcpApiKey(
  name: string,
  apiKey: string,
  apiKeyParam?: string
): Promise<UpdateApiKeyResponse> {
  return await apiClient
    .post(`api/mcp/servers/${encodeURIComponent(name)}/api-key`, {
      json: { api_key: apiKey, api_key_param: apiKeyParam },
    })
    .json<UpdateApiKeyResponse>();
}
