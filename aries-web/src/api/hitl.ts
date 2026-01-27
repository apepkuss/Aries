import { apiClient } from './client';
import type {
  HitlRequest,
  HitlRespondRequest,
  HitlRespondResponse,
  HitlPendingListResponse,
  HitlHistoryResponse,
} from './types';

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

  return apiClient
    .get('v1/hitl/pending', { searchParams })
    .json<HitlPendingListResponse>();
}

/**
 * Get details of a specific HITL request
 * @param requestId - The HITL request ID
 */
export async function getRequestDetail(requestId: string): Promise<HitlRequest> {
  return apiClient.get(`v1/hitl/${requestId}`).json<HitlRequest>();
}

/**
 * Respond to a HITL request
 * @param requestId - The HITL request ID
 * @param response - The response action
 */
export async function respondToRequest(
  requestId: string,
  response: HitlRespondRequest
): Promise<HitlRespondResponse> {
  return apiClient
    .post(`v1/hitl/${requestId}/respond`, {
      json: response,
    })
    .json<HitlRespondResponse>();
}

/**
 * Cancel a pending HITL request
 * @param requestId - The HITL request ID
 */
export async function cancelRequest(requestId: string): Promise<void> {
  await apiClient.delete(`v1/hitl/${requestId}`);
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
    .get('v1/hitl/history', {
      searchParams: Object.keys(searchParams).length > 0 ? searchParams : undefined,
    })
    .json<HitlHistoryResponse>();
}

/**
 * Helper: Approve a HITL request
 */
export async function approveRequest(requestId: string): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'approve' },
  });
}

/**
 * Helper: Reject a HITL request
 */
export async function rejectRequest(
  requestId: string,
  reason?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'reject', reason },
  });
}

/**
 * Helper: Modify and approve a HITL request
 */
export async function modifyRequest(
  requestId: string,
  modifications: Record<string, unknown>
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'modify', modifications },
  });
}

/**
 * Helper: Abort a HITL request
 */
export async function abortRequest(
  requestId: string,
  reason?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'abort', reason },
  });
}

/**
 * Helper: Provide clarification for a HITL request
 */
export async function clarifyRequest(
  requestId: string,
  selectedOption?: number,
  input?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'clarify', selected_option: selectedOption, input },
  });
}

/**
 * Helper: Provide feedback for a HITL request
 */
export async function provideFeedback(
  requestId: string,
  rating?: number,
  comment?: string
): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'provide_feedback', rating, comment },
  });
}

/**
 * Helper: Resume from a pause HITL request
 */
export async function resumeRequest(requestId: string): Promise<HitlRespondResponse> {
  return respondToRequest(requestId, {
    response: { action: 'resume' },
  });
}
