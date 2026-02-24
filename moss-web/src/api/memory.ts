import { apiClient, getCurrentUserId, ApiError } from './client';
import type {
  Conversation,
  ConversationListResponse,
  ConversationHistoryResponse,
} from './types';

/**
 * Get all conversations for the current user
 * Returns empty list if memory feature is not enabled (404)
 */
export async function getConversations(): Promise<ConversationListResponse> {
  const userId = getCurrentUserId();
  try {
    return await apiClient
      .get(`v1/memory/users/${userId}/conversations`, {
        retry: 0, // Don't retry - memory may not be enabled
      })
      .json<ConversationListResponse>();
  } catch (err) {
    // If memory feature is not enabled, return empty list
    if (err instanceof ApiError && err.status === 404) {
      return { conversations: [], total: 0 };
    }
    throw err;
  }
}

/**
 * Get conversation history (messages)
 */
export async function getConversationHistory(
  conversationId: string
): Promise<ConversationHistoryResponse> {
  return apiClient
    .get(`v1/memory/conversations/${conversationId}/history`)
    .json<ConversationHistoryResponse>();
}

/**
 * Delete a conversation
 */
export async function deleteConversation(conversationId: string): Promise<void> {
  await apiClient.delete(`v1/memory/conversations/${conversationId}`);
}

/**
 * Rename a conversation
 */
export async function renameConversation(
  conversationId: string,
  title: string
): Promise<Conversation> {
  return apiClient
    .patch(`v1/memory/conversations/${conversationId}`, {
      json: { title },
    })
    .json<Conversation>();
}
