import { create } from 'zustand';
import { getSkills, reloadSkills, installSkill as installSkillApi, setSkillEnabled } from '@/api/skills';
import type { SkillSummary } from '@/api/types';

interface SkillsState {
  skills: SkillSummary[];
  isLoading: boolean;
  error: string | null;
  isInstalling: boolean;
  installError: string | null;
  fetchSkills: () => Promise<void>;
  installSkill: (url: string, name?: string, envVars?: Record<string, string>) => Promise<boolean>;
  toggleSkill: (name: string, enabled: boolean) => Promise<void>;
  clearInstallError: () => void;
}

export const useSkillsStore = create<SkillsState>((set, get) => ({
  skills: [],
  isLoading: false,
  error: null,
  isInstalling: false,
  installError: null,

  fetchSkills: async () => {
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      await reloadSkills();
      const response = await getSkills();
      set({ skills: response.skills, isLoading: false });
    } catch (err) {
      console.warn('Failed to fetch skills:', err instanceof Error ? err.message : err);
      set({ skills: [], isLoading: false, error: null });
    }
  },

  installSkill: async (url: string, name?: string, envVars?: Record<string, string>) => {
    set({ isInstalling: true, installError: null });
    try {
      const response = await installSkillApi(url, name, envVars);
      if (response.success) {
        await get().fetchSkills();
        set({ isInstalling: false });
        return true;
      } else {
        set({ isInstalling: false, installError: response.message });
        return false;
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Installation failed';
      set({ isInstalling: false, installError: message });
      return false;
    }
  },

  toggleSkill: async (name: string, enabled: boolean) => {
    // Optimistic update
    set((state) => ({
      skills: state.skills.map((s) =>
        s.name === name ? { ...s, enabled } : s
      ),
    }));

    try {
      await setSkillEnabled(name, enabled);
    } catch {
      // Revert on failure
      set((state) => ({
        skills: state.skills.map((s) =>
          s.name === name ? { ...s, enabled: !enabled } : s
        ),
      }));
    }
  },

  clearInstallError: () => set({ installError: null }),
}));
