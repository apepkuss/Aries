import { useEffect } from 'react';
import { useUIStore, applyTheme } from '@/stores';

export function useTheme() {
  const { theme } = useUIStore();

  useEffect(() => {
    // Apply theme immediately
    applyTheme(theme);

    // Listen for system theme changes
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleChange = () => {
      if (theme === 'system') {
        applyTheme('system');
      }
    };

    mediaQuery.addEventListener('change', handleChange);
    return () => mediaQuery.removeEventListener('change', handleChange);
  }, [theme]);
}
