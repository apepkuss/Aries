import { create } from 'zustand';
import { getSkills } from '@/api/skills';
import type { SkillSummary } from '@/api/types';

interface SkillsState {
  skills: SkillSummary[];
  isLoading: boolean;
  error: string | null;
  fetchSkills: () => Promise<void>;
}

export const useSkillsStore = create<SkillsState>((set, get) => ({
  skills: [],
  isLoading: false,
  error: null,

  fetchSkills: async () => {
    if (get().isLoading) return;

    set({ isLoading: true, error: null });
    try {
      const response = await getSkills();
      set({ skills: response.skills, isLoading: false });
    } catch (err) {
      console.warn('Failed to fetch skills:', err instanceof Error ? err.message : err);
      set({ skills: [], isLoading: false, error: null });
    }
  },
}));
