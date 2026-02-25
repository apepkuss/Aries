import { apiClient, ApiError } from './client';
import type {
  SkillListResponse,
  InstallSkillResponse,
  SkillEnvResponse,
  UpdateSkillEnvResponse,
} from './types';

/**
 * Enable or disable a skill.
 */
export async function setSkillEnabled(
  name: string,
  enabled: boolean,
): Promise<void> {
  await apiClient
    .put(`api/skills/${encodeURIComponent(name)}/enabled`, {
      json: { enabled },
    });
}

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

/**
 * Reload all skills from the skills directory.
 * Clears the skill cache and re-scans ~/.moss/skills for new/updated skills.
 * Returns empty result if skills system is not enabled (503).
 */
export async function reloadSkills(): Promise<void> {
  try {
    await apiClient.post('api/skills/reload', { retry: 0 });
  } catch (err) {
    if (err instanceof ApiError && (err.status === 503 || err.status === 404)) {
      return;
    }
    throw err;
  }
}

/**
 * Install a skill from a URL with optional initial environment variables.
 */
export async function installSkill(
  url: string,
  name?: string,
  envVars?: Record<string, string>,
): Promise<InstallSkillResponse> {
  const body: Record<string, unknown> = { url };
  if (name) {
    body.name = name;
  }
  if (envVars && Object.keys(envVars).length > 0) {
    body.env_vars = envVars;
  }
  return await apiClient
    .post('api/skills/install', { json: body, timeout: 120000 })
    .json<InstallSkillResponse>();
}

/**
 * Get environment variables for a skill.
 */
export async function getSkillEnv(name: string): Promise<SkillEnvResponse> {
  return await apiClient
    .get(`api/skills/${encodeURIComponent(name)}/env`, { retry: 0 })
    .json<SkillEnvResponse>();
}

/**
 * Update environment variables for a skill.
 */
export async function updateSkillEnv(
  name: string,
  envVars: Record<string, string>,
): Promise<UpdateSkillEnvResponse> {
  return await apiClient
    .put(`api/skills/${encodeURIComponent(name)}/env`, {
      json: { env_vars: envVars },
    })
    .json<UpdateSkillEnvResponse>();
}
