import { useSyncExternalStore } from 'react';
import { useAppStore } from '../stores';
import type { ResolvedTheme, ThemePreference } from '../types';

const DARK_SCHEME_QUERY = '(prefers-color-scheme: dark)';

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined') {
    return 'light';
  }

  return window.matchMedia(DARK_SCHEME_QUERY).matches ? 'dark' : 'light';
}

function subscribeToSystemTheme(onStoreChange: () => void) {
  const mediaQuery = window.matchMedia(DARK_SCHEME_QUERY);

  mediaQuery.addEventListener('change', onStoreChange);
  return () => mediaQuery.removeEventListener('change', onStoreChange);
}

function resolveTheme(
  themePreference: ThemePreference | undefined,
  systemTheme: ResolvedTheme
): ResolvedTheme {
  if (themePreference === 'dark' || themePreference === 'light') {
    return themePreference;
  }

  return systemTheme;
}

export function useResolvedTheme(): ResolvedTheme {
  const themePreference = useAppStore((s) => s.config?.theme_preference ?? 'system');
  const systemTheme = useSyncExternalStore<ResolvedTheme>(
    subscribeToSystemTheme,
    getSystemTheme,
    () => 'light'
  );

  return resolveTheme(themePreference, systemTheme);
}
