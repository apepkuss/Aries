import { apiClient, ApiError } from './client';
import type { SkillListResponse } from './types';

/**
 * Get all loaded skills.
 * Returns empty list if skills system is not enabled (503).
 */
export async function getSkills(): Promise<SkillListResponse> {
  try {
    return await apiClient
      .get('api/skills', { retry: 0 })
      .json<SkillListResponse>();
  } catch (err) {
    if (err instanceof ApiError && (err.status === 503 || err.status === 404)) {
      return { skills: [], total: 0 };
    }
    throw err;
  }
}
