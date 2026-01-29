/**
 * Models API - Fetch available models from the backend
 */

export interface Model {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

export interface ListModelsResponse {
  object: string;
  data: Model[];
}

/**
 * Fetch available models from the /v1/models endpoint
 */
export async function fetchModels(): Promise<ListModelsResponse> {
  const response = await fetch('/v1/models');
  if (!response.ok) {
    throw new Error(`Failed to fetch models: ${response.statusText}`);
  }
  return response.json();
}
