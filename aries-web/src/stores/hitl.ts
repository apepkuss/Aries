import { create } from 'zustand';
import type {
  HitlRequest,
  UIHitlRequest,
  HitlRequestEvent,
  HitlStatusEvent,
  HitlTimeoutWarningEvent,
  HitlRequestStatus,
  HitlRespondResponse,
  HitlRequestType,
  HitlRiskLevel,
} from '@/api/types';
import {
  getPendingRequests,
  getRequestDetail,
  approveRequest,
  rejectRequest,
  modifyRequest,
  abortRequest,
  clarifyRequest,
  provideFeedback,
  resumeRequest,
  cancelRequest,
} from '@/api/hitl';

interface HitlState {
  // Pending requests (keyed by request ID)
  pendingRequests: Map<string, UIHitlRequest>;

  // Currently active/focused request
  activeRequestId: string | null;

  // Loading states
  isLoading: boolean;
  isResponding: boolean;

  // Error state
  error: string | null;

  // Actions
  fetchPendingRequests: (conversationId?: string) => Promise<void>;
  fetchRequestDetail: (requestId: string) => Promise<UIHitlRequest | null>;

  // Response actions
  approve: (requestId: string) => Promise<HitlRespondResponse | null>;
  reject: (requestId: string, reason?: string) => Promise<HitlRespondResponse | null>;
  modify: (
    requestId: string,
    modifications: Record<string, unknown>
  ) => Promise<HitlRespondResponse | null>;
  abort: (requestId: string, reason?: string) => Promise<HitlRespondResponse | null>;
  clarify: (
    requestId: string,
    selectedOption?: number,
    input?: string
  ) => Promise<HitlRespondResponse | null>;
  feedback: (
    requestId: string,
    rating?: number,
    comment?: string
  ) => Promise<HitlRespondResponse | null>;
  resume: (requestId: string) => Promise<HitlRespondResponse | null>;
  cancel: (requestId: string) => Promise<boolean>;

  // SSE event handlers
  handleRequestEvent: (event: HitlRequestEvent) => void;
  handleStatusEvent: (event: HitlStatusEvent) => void;
  handleTimeoutWarning: (event: HitlTimeoutWarningEvent) => void;

  // UI actions
  setActiveRequest: (requestId: string | null) => void;
  toggleRequestExpanded: (requestId: string) => void;
  clearError: () => void;
  clearRequests: () => void;

  // Internal helper
  _respond: (
    requestId: string,
    action: () => Promise<HitlRespondResponse>
  ) => Promise<HitlRespondResponse | null>;
}

// Convert API request to UI request
function toUIRequest(request: HitlRequest): UIHitlRequest {
  const expiresAt = new Date(request.expires_at).getTime();
  const now = Date.now();
  const remainingSeconds = Math.max(0, Math.floor((expiresAt - now) / 1000));

  return {
    ...request,
    remainingSeconds,
    expanded: false,
    isResponding: false,
  };
}

// Create a placeholder request type for SSE events (full data fetched separately)
function createPlaceholderRequestType(
  type: 'confirmation' | 'clarification' | 'feedback' | 'pause',
  eventData?: {
    summary?: string;
    tool_name?: string;
    risk_level?: string;
  }
): HitlRequestType {
  switch (type) {
    case 'confirmation':
      return {
        type: 'confirmation',
        data: {
          summary: eventData?.summary || '',
          tool_name: eventData?.tool_name || '',
          tool_args: {},
          risk_level: (eventData?.risk_level as HitlRiskLevel) || 'medium',
          preview: { type: 'generic', title: '', description: '' },
          risk_factors: [],
          allow_modification: false,
          modifiable_fields: [],
        },
      };
    case 'clarification':
      return {
        type: 'clarification',
        data: {
          question: '',
          options: [],
          allow_free_input: true,
        },
      };
    case 'feedback':
      return {
        type: 'feedback',
        data: {
          summary: '',
          rating_requested: false,
          comment_requested: false,
        },
      };
    case 'pause':
      return {
        type: 'pause',
        data: {
          reason: 'user_requested',
          current_state: '',
          completed_steps: [],
          pending_steps: [],
        },
      };
  }
}

export const useHitlStore = create<HitlState>((set, get) => ({
  // Initial state
  pendingRequests: new Map(),
  activeRequestId: null,
  isLoading: false,
  isResponding: false,
  error: null,

  // Fetch all pending requests
  fetchPendingRequests: async (conversationId?: string) => {
    set({ isLoading: true, error: null });

    try {
      const response = await getPendingRequests(conversationId);
      const requests = new Map<string, UIHitlRequest>();

      for (const req of response.requests) {
        requests.set(req.id, toUIRequest(req));
      }

      set({ pendingRequests: requests, isLoading: false });
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to fetch pending requests',
        isLoading: false,
      });
    }
  },

  // Fetch single request detail
  fetchRequestDetail: async (requestId: string) => {
    try {
      const request = await getRequestDetail(requestId);
      const uiRequest = toUIRequest(request);

      set((state) => ({
        pendingRequests: new Map(state.pendingRequests).set(requestId, uiRequest),
      }));

      return uiRequest;
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to fetch request detail',
      });
      return null;
    }
  },

  // Response helper
  _respond: async (
    requestId: string,
    action: () => Promise<HitlRespondResponse>
  ): Promise<HitlRespondResponse | null> => {
    const { pendingRequests } = get();
    const request = pendingRequests.get(requestId);

    if (!request) {
      set({ error: 'Request not found' });
      return null;
    }

    // Set responding state
    set((state) => ({
      isResponding: true,
      pendingRequests: new Map(state.pendingRequests).set(requestId, {
        ...request,
        isResponding: true,
      }),
    }));

    try {
      const response = await action();

      // Remove from pending if terminal status
      const terminalStatuses: HitlRequestStatus[] = [
        'approved',
        'rejected',
        'modified',
        'expired',
        'cancelled',
        'completed',
      ];

      set((state) => {
        const newRequests = new Map(state.pendingRequests);
        if (terminalStatuses.includes(response.status)) {
          newRequests.delete(requestId);
        } else {
          const req = newRequests.get(requestId);
          if (req) {
            newRequests.set(requestId, {
              ...req,
              status: response.status,
              isResponding: false,
            });
          }
        }
        return {
          pendingRequests: newRequests,
          isResponding: false,
          activeRequestId:
            state.activeRequestId === requestId ? null : state.activeRequestId,
        };
      });

      return response;
    } catch (err) {
      // Check if the error is a 410 Gone or 404 Not Found (request expired/not found)
      // ApiError has a status property, but we also check message for safety
      const errorStatus = (err as { status?: number }).status;
      const isGoneError =
        errorStatus === 410 ||
        errorStatus === 404 ||
        (err instanceof Error &&
          (err.message.includes('410') ||
            err.message.includes('Gone') ||
            err.message.includes('expired') ||
            err.message.includes('not found')));

      set((state) => {
        const newRequests = new Map(state.pendingRequests);

        if (isGoneError) {
          // Request expired or not found - remove it from pending
          newRequests.delete(requestId);
          console.log(`[HITL] Request ${requestId} expired or not found (status: ${errorStatus}), removing from pending list`);
        } else {
          // Other error - keep request but reset responding state
          const req = newRequests.get(requestId);
          if (req) {
            newRequests.set(requestId, { ...req, isResponding: false });
          }
        }

        return {
          error: isGoneError
            ? 'Request has expired'
            : err instanceof Error
              ? err.message
              : 'Failed to respond',
          isResponding: false,
          pendingRequests: newRequests,
          activeRequestId: isGoneError && state.activeRequestId === requestId
            ? null
            : state.activeRequestId,
        };
      });
      return null;
    }
  },

  // Approve request
  approve: async (requestId: string) => {
    return get()._respond(requestId, () => approveRequest(requestId));
  },

  // Reject request
  reject: async (requestId: string, reason?: string) => {
    return get()._respond(requestId, () => rejectRequest(requestId, reason));
  },

  // Modify request
  modify: async (requestId: string, modifications: Record<string, unknown>) => {
    return get()._respond(requestId, () => modifyRequest(requestId, modifications));
  },

  // Abort request
  abort: async (requestId: string, reason?: string) => {
    return get()._respond(requestId, () => abortRequest(requestId, reason));
  },

  // Clarify request
  clarify: async (requestId: string, selectedOption?: number, input?: string) => {
    return get()._respond(requestId, () =>
      clarifyRequest(requestId, selectedOption, input)
    );
  },

  // Provide feedback
  feedback: async (requestId: string, rating?: number, comment?: string) => {
    return get()._respond(requestId, () => provideFeedback(requestId, rating, comment));
  },

  // Resume request
  resume: async (requestId: string) => {
    return get()._respond(requestId, () => resumeRequest(requestId));
  },

  // Cancel request
  cancel: async (requestId: string) => {
    try {
      await cancelRequest(requestId);

      set((state) => {
        const newRequests = new Map(state.pendingRequests);
        newRequests.delete(requestId);
        return {
          pendingRequests: newRequests,
          activeRequestId:
            state.activeRequestId === requestId ? null : state.activeRequestId,
        };
      });

      return true;
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to cancel request',
      });
      return false;
    }
  },

  // Handle SSE: new request event
  handleRequestEvent: (event: HitlRequestEvent) => {
    // Create a minimal request from the event
    // Note: Some fields are populated from event, rest will be filled when fetchRequestDetail completes
    const requestType = event.request_type as 'confirmation' | 'clarification' | 'feedback' | 'pause';
    const request: UIHitlRequest = {
      id: event.request_id,
      conversation_id: event.conversation_id,
      user_id: '', // Will be filled when we fetch detail
      request_type: createPlaceholderRequestType(requestType, {
        summary: event.summary,
        tool_name: event.tool_name,
        risk_level: event.risk_level,
      }),
      status: 'pending',
      created_at: new Date().toISOString(),
      expires_at: event.expires_at,
      timeout_behavior: event.timeout_behavior,
      remainingSeconds: Math.max(
        0,
        Math.floor((new Date(event.expires_at).getTime() - Date.now()) / 1000)
      ),
      expanded: false,
      isResponding: false,
      subtask_id: event.subtask_id,
      subagent_id: event.subagent_id,
    };

    set((state) => ({
      pendingRequests: new Map(state.pendingRequests).set(event.request_id, request),
      // Auto-set as active if no active request
      activeRequestId: state.activeRequestId ?? event.request_id,
    }));

    // Fetch full detail
    get().fetchRequestDetail(event.request_id);
  },

  // Handle SSE: status change event
  handleStatusEvent: (event: HitlStatusEvent) => {
    const terminalStatuses: HitlRequestStatus[] = [
      'approved',
      'rejected',
      'modified',
      'expired',
      'cancelled',
      'completed',
    ];

    set((state) => {
      const newRequests = new Map(state.pendingRequests);

      if (terminalStatuses.includes(event.status)) {
        newRequests.delete(event.request_id);
      } else {
        const request = newRequests.get(event.request_id);
        if (request) {
          newRequests.set(event.request_id, {
            ...request,
            status: event.status,
          });
        }
      }

      return {
        pendingRequests: newRequests,
        activeRequestId:
          terminalStatuses.includes(event.status) &&
          state.activeRequestId === event.request_id
            ? null
            : state.activeRequestId,
      };
    });
  },

  // Handle SSE: timeout warning event
  handleTimeoutWarning: (event: HitlTimeoutWarningEvent) => {
    set((state) => {
      const newRequests = new Map(state.pendingRequests);
      const request = newRequests.get(event.request_id);

      if (request) {
        newRequests.set(event.request_id, {
          ...request,
          remainingSeconds: event.remaining_seconds,
        });
      }

      return { pendingRequests: newRequests };
    });
  },

  // Set active request
  setActiveRequest: (requestId: string | null) => {
    set({ activeRequestId: requestId });
  },

  // Toggle request expanded state
  toggleRequestExpanded: (requestId: string) => {
    set((state) => {
      const newRequests = new Map(state.pendingRequests);
      const request = newRequests.get(requestId);

      if (request) {
        newRequests.set(requestId, {
          ...request,
          expanded: !request.expanded,
        });
      }

      return { pendingRequests: newRequests };
    });
  },

  // Clear error
  clearError: () => {
    set({ error: null });
  },

  // Clear all requests
  clearRequests: () => {
    set({
      pendingRequests: new Map(),
      activeRequestId: null,
    });
  },
}));

// Selector hooks
export const useActiveHitlRequest = () => {
  const activeRequestId = useHitlStore((state) => state.activeRequestId);
  const pendingRequests = useHitlStore((state) => state.pendingRequests);
  return activeRequestId ? pendingRequests.get(activeRequestId) : undefined;
};

export const usePendingHitlRequests = () => {
  const pendingRequests = useHitlStore((state) => state.pendingRequests);
  return Array.from(pendingRequests.values());
};

export const useHitlRequestCount = () => {
  const pendingRequests = useHitlStore((state) => state.pendingRequests);
  return pendingRequests.size;
};
