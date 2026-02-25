import { apiClient } from './client';
import type {
  ClawHubSearchResponse,
  ClawHubBrowseResponse,
  ClawHubSkill,
  ClawHubInstallRequest,
  InstallSkillResponse,
} from './types';

/**
 * Search ClawHub skills using semantic search.
 */
export async function searchClawHub(
  q: string,
  limit: number = 20,
): Promise<ClawHubSearchResponse> {
  return await apiClient
    .get('api/clawhub/search', {
      searchParams: { q, limit: limit.toString() },
      retry: 0,
      timeout: 15000,
    })
    .json<ClawHubSearchResponse>();
}

/**
 * Browse ClawHub skills with pagination and sorting.
 */
export async function browseClawHub(params: {
  limit?: number;
  cursor?: string;
  sort?: string;
}): Promise<ClawHubBrowseResponse> {
  const searchParams: Record<string, string> = {};
  if (params.limit) searchParams.limit = params.limit.toString();
  if (params.cursor) searchParams.cursor = params.cursor;
  if (params.sort) searchParams.sort = params.sort;

  return await apiClient
    .get('api/clawhub/skills', {
      searchParams,
      retry: 0,
      timeout: 15000,
    })
    .json<ClawHubBrowseResponse>();
}

/**
 * Get detailed information about a specific ClawHub skill.
 */
export async function getClawHubSkill(
  slug: string,
): Promise<ClawHubSkill> {
  return await apiClient
    .get(`api/clawhub/skills/${encodeURIComponent(slug)}`, {
      retry: 0,
      timeout: 15000,
    })
    .json<ClawHubSkill>();
}

/**
 * Install a skill from ClawHub.
 */
export async function installClawHubSkill(
  request: ClawHubInstallRequest,
): Promise<InstallSkillResponse> {
  return await apiClient
    .post('api/clawhub/install', {
      json: request,
      timeout: 120000,
    })
    .json<InstallSkillResponse>();
}
