/**
 * Admin API — Server registration and management
 */

import { apiClient } from './client';

export interface RegisterServerRequest {
  url: string;
  kind: string;
  api_key?: string;
}

export interface RegisterServerResponse {
  id: string;
  url: string;
  kind: string;
}

export interface UnregisterServerResponse {
  message: string;
  id: string;
}

/**
 * Register a downstream server with the backend
 */
export async function registerServer(
  request: RegisterServerRequest
): Promise<RegisterServerResponse> {
  return apiClient
    .post('admin/servers/register', { json: request })
    .json<RegisterServerResponse>();
}

/**
 * Unregister a downstream server from the backend
 */
export async function unregisterServer(
  serverId: string
): Promise<UnregisterServerResponse> {
  return apiClient
    .post('admin/servers/unregister', { json: { server_id: serverId } })
    .json<UnregisterServerResponse>();
}
