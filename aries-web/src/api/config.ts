import { apiClient } from './client';
import type {
  SanitizedConfig,
  ConfigUpdateRequest,
  ConfigUpdateResponse,
  ConfigSchemaResponse,
  TestChatServiceRequest,
  TestChatServiceResponse,
} from './types';

/**
 * Get current configuration (sensitive fields are redacted)
 */
export async function getConfig(): Promise<SanitizedConfig> {
  return apiClient.get('v1/config').json<SanitizedConfig>();
}

/**
 * Update configuration
 * Supports partial updates - only specified fields will be changed
 */
export async function updateConfig(
  updates: ConfigUpdateRequest
): Promise<ConfigUpdateResponse> {
  return apiClient
    .post('v1/config', {
      json: updates,
    })
    .json<ConfigUpdateResponse>();
}

/**
 * Get configuration schema
 * Returns field types, constraints, and descriptions
 */
export async function getConfigSchema(): Promise<ConfigSchemaResponse> {
  return apiClient.get('v1/config/schema').json<ConfigSchemaResponse>();
}

/**
 * Test chat service connectivity
 * Attempts to connect to the specified URL and fetch available models
 */
export async function testChatService(
  request: TestChatServiceRequest
): Promise<TestChatServiceResponse> {
  return apiClient
    .post('v1/config/test-chat', {
      json: request,
    })
    .json<TestChatServiceResponse>();
}
