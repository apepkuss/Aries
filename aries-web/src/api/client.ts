import ky from 'ky';

// Detect if running inside Electron
export const isElectron = (): boolean => {
  return (window as unknown as Record<string, unknown>)?.electronAPI !== undefined;
};

// Generate a unique request ID
function generateRequestId(): string {
  return `req_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
}

// Get user ID from localStorage or generate a new one
function getUserId(): string {
  const key = 'aries_user_id';
  let userId = localStorage.getItem(key);
  if (!userId) {
    userId = `user_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
    localStorage.setItem(key, userId);
  }
  return userId;
}

// Create the base API client
export const apiClient = ky.create({
  prefixUrl: '',
  timeout: 30000,
  hooks: {
    beforeRequest: [
      (request) => {
        request.headers.set('X-User-ID', getUserId());
        request.headers.set('X-Request-ID', generateRequestId());
      },
    ],
    afterResponse: [
      async (_request, _options, response) => {
        if (!response.ok) {
          const error = await response.json().catch(() => ({}));
          throw new ApiError(
            (error as { message?: string }).message || response.statusText,
            response.status,
            error
          );
        }
        return response;
      },
    ],
  },
});

// Custom API error class
export class ApiError extends Error {
  status: number;
  data?: unknown;

  constructor(message: string, status: number, data?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.data = data;
  }
}

// Helper to get current user ID
export function getCurrentUserId(): string {
  return getUserId();
}
