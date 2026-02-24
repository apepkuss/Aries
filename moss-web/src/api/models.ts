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
 * @param service - Which service to query models from ('chat', 'privacy_chat', or 'embeddings')
 */
export async function fetchModels(service?: 'chat' | 'privacy_chat' | 'embeddings'): Promise<ListModelsResponse> {
  const params = service ? `?service=${service}` : '';
  const response = await fetch(`/v1/models${params}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch models: ${response.statusText}`);
  }
  return response.json();
}
