import { apiClient, getCurrentUserId } from './client';
import type {
  HitlRequest,
  HitlRequestFromApi,
  HitlRequestType,
  HitlTimeoutBehavior,
  HitlRespondResponse,
  HitlPendingListResponse,
  HitlHistoryResponse,
} from './types';

/**
 * Convert backend API response to frontend HitlRequest format
 */
function convertApiResponseToHitlRequest(apiResponse: HitlRequestFromApi): HitlRequest {
  // Debug: log raw API response
  console.log('[HITL API] Raw API response for', apiResponse.id, ':', JSON.stringify(apiResponse, null, 2));

  // Convert request_type string + details to the { type, data } format
  const requestType: HitlRequestType = {
    type: apiResponse.request_type as 'confirmation' | 'clarification' | 'feedback' | 'pause' | 'privacy_mode_confirmation',
    data: apiResponse.details,
  } as HitlRequestType;

  console.log('[HITL API] Converted request_type:', requestType);

  return {
    id: apiResponse.id,
    conversation_id: apiResponse.conversation_id,
    user_id: apiResponse.user_id,
    request_type: requestType,
    status: apiResponse.status,
    created_at: apiResponse.created_at,
    expires_at: apiResponse.expires_at,
    timeout_behavior: apiResponse.timeout_behavior as HitlTimeoutBehavior,
    subtask_id: apiResponse.subtask_id,
    subagent_id: apiResponse.subagent_id,
  };
}

/**
 * Backend request format for HITL respond endpoint
 */
interface BackendRespondRequest {
  user_id: string;
  response_type: string;
  data?: Record<string, unknown>;
}

/** Raw API response from list pending requests */
interface HitlPendingListResponseFromApi {
  requests: HitlRequestFromApi[];
  total: number;
}

/**
 * Get all pending HITL requests for the current conversation
 * @param conversationId - Optional conversation ID to filter by
 */
export async function getPendingRequests(
  conversationId?: string
): Promise<HitlPendingListResponse> {
  const searchParams = conversationId
    ? { conversation_id: conversationId }
    : undefined;

  const apiResponse = await apiClient
    .get('api/hitl/pending', { searchParams })
    .json<HitlPendingListResponseFromApi>();

  // Convert API format to frontend format
  return {
    requests: apiResponse.requests.map(convertApiResponseToHitlRequest),
    total: apiResponse.total,
  };
}

/**
 * Get details of a specific HITL request
 * @param requestId - The HITL request ID
 */
export async function getRequestDetail(requestId: string): Promise<HitlRequest> {
  const apiResponse = await apiClient
    .get(`api/hitl/requests/${requestId}`)
    .json<HitlRequestFromApi>();
  return convertApiResponseToHitlRequest(apiResponse);
}

/**
 * Respond to a HITL request (internal)
 * @param requestId - The HITL request ID
 * @param responseType - The response type (approve, reject, modify, abort, clarify, feedback, resume)
 * @param data - Optional additional data
 */
async function respondToRequest(
  requestId: string,
  responseType: string,
  data?: Record<string, unknown>
): Promise<HitlRespondResponse> {
  const request: BackendRespondRequest = {
    user_id: getCurrentUserId(),
    response_type: responseType,
    data,
  };

  return apiClient
    .post(`api/hitl/requests/${requestId}/respond`, {
      json: request,
    })
    .json<HitlRespondResponse>();
}

/**
 * Cancel a pending HITL request
 * @param requestId - The HITL request ID
 */
export async function cancelRequest(requestId: string): Promise<void> {
  const userId = getCurrentUserId();
  await apiClient.delete(`api/hitl/requests/${requestId}`, {
    searchParams: { user_id: userId },
  });
}

/**
 * Get HITL request history
 * @param options - Pagination and filter options
 */
export async function getRequestHistory(options?: {
  conversationId?: string;
  page?: number;
  pageSize?: number;
  status?: string;
}): Promise<HitlHistoryResponse> {
  const searchParams: Record<string, string | number> = {};

  if (options?.conversationId) {
    searchParams.conversation_id = options.conversationId;
  }
  if (options?.page !== undefined) {
    searchParams.page = options.page;
  }
  if (options?.pageSize !== undefined) {
    searchParams.page_size = options.pageSize;
  }
  if (options?.status) {
    searchParams.status = options.status;
  }

  return apiClient
    .get('api/hitl/stats', {
      searchParams: Object.keys(searchParams).length > 0 ? searchParams : undefined,
    })
    .json<HitlHistoryResponse>();
}

/**
 * Helper: Approve a HITL request
 */
export async function approveRequest(requestId: string): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, 'approve');
}

/**
 * Helper: Reject a HITL request
 */
export async function rejectRequest(
  requestId: string,
  reason?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, 'reject', reason ? { reason } : undefined);
}

/**
 * Helper: Modify and approve a HITL request
 */
export async function modifyRequest(
  requestId: string,
  modifications: Record<string, unknown>
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, 'modify', { modifications });
}

/**
 * Helper: Abort a HITL request
 */
export async function abortRequest(
  requestId: string,
  reason?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, 'abort', reason ? { reason } : undefined);
}

/**
 * Helper: Provide clarification for a HITL request
 */
export async function clarifyRequest(
  requestId: string,
  selectedOption?: number,
  input?: string
): Promise<HitlRespondResponse> {
  const data: Record<string, unknown> = {};
  if (selectedOption !== undefined) {
    data.selected_option = selectedOption;
  }
  if (input !== undefined) {
    data.input = input;
  }
  return respondToRequest(requestId, 'clarify', Object.keys(data).length > 0 ? data : undefined);
}

/**
 * Helper: Provide feedback for a HITL request
 */
export async function provideFeedback(
  requestId: string,
  rating?: number,
  comment?: string
): Promise<HitlRespondResponse> {
  const data: Record<string, unknown> = {};
  if (rating !== undefined) {
    data.rating = rating;
  }
  if (comment !== undefined) {
    data.comment = comment;
  }
  return respondToRequest(requestId, 'feedback', Object.keys(data).length > 0 ? data : undefined);
}

/**
 * Helper: Resume from a pause HITL request
 */
export async function resumeRequest(requestId: string): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, 'resume');
}

/**
 * Helper: Respond to privacy mode confirmation request
 */
export async function respondPrivacyModeChoice(
  requestId: string,
  usePrivacyMode: boolean,
  rememberChoice?: boolean
): Promise<HitlRespondResponse> {
  const data: Record<string, unknown> = {
    use_privacy_mode: usePrivacyMode,
  };
  if (rememberChoice !== undefined) {
    data.remember_choice = rememberChoice;
  }
  return respondToRequest(requestId, 'privacy_mode_choice', data);
}
