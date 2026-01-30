import { apiClient, getCurrentUserId, ApiError } from './client';
import type {
  SessionListResponse,
  SessionDetailResponse,
  SessionDeleteResponse,
} from './types';

/**
 * Get all sessions for the current user.
 * Returns empty list if session feature is not enabled (503).
 */
export async function getSessions(): Promise<SessionListResponse> {
  const userId = getCurrentUserId();
  try {
    return await apiClient
      .get(`v1/sessions`, {
        searchParams: { user_id: userId },
        retry: 0, // Don't retry - session feature may not be enabled
      })
      .json<SessionListResponse>();
  } catch (err) {
    // If session feature is not enabled, return empty list
    if (err instanceof ApiError && (err.status === 503 || err.status === 404)) {
      return { sessions: [], total: 0 };
    }
    throw err;
  }
}

/**
 * Get all records from a specific session.
 */
export async function getSessionDetail(
  sessionId: string
): Promise<SessionDetailResponse> {
  const userId = getCurrentUserId();
  return apiClient
    .get(`v1/sessions/${sessionId}`, {
      searchParams: { user_id: userId },
    })
    .json<SessionDetailResponse>();
}

/**
 * Delete a specific session.
 */
export async function deleteSession(
  sessionId: string
): Promise<SessionDeleteResponse> {
  const userId = getCurrentUserId();
  return apiClient
    .delete(`v1/sessions/${sessionId}`, {
      searchParams: { user_id: userId },
    })
    .json<SessionDeleteResponse>();
}
