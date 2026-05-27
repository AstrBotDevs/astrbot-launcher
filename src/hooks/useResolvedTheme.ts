import { useSyncExternalStore } from 'react';
import { useAppStore } from '../stores';
import type { ResolvedTheme, ThemePreference } from '../types';

const DARK_SCHEME_QUERY = '(prefers-color-scheme: dark)';

const getDefaultTheme = (): ResolvedTheme => 'light';
const subscribeToNoop = () => () => {};

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined' || !window.matchMedia) {
    return getDefaultTheme();
  }

  return window.matchMedia(DARK_SCHEME_QUERY).matches ? 'dark' : 'light';
}

function subscribeToSystemTheme(onStoreChange: () => void) {
  if (typeof window === 'undefined' || !window.matchMedia) {
    return subscribeToNoop();
  }

  const mediaQuery = window.matchMedia(DARK_SCHEME_QUERY);

  if (typeof mediaQuery.addEventListener === 'function') {
    mediaQuery.addEventListener('change', onStoreChange);
    return () => mediaQuery.removeEventListener('change', onStoreChange);
  }

  mediaQuery.addListener(onStoreChange);
  return () => mediaQuery.removeListener(onStoreChange);
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
  const shouldResolveSystemTheme = themePreference === 'system';
  const systemTheme = useSyncExternalStore<ResolvedTheme>(
    shouldResolveSystemTheme ? subscribeToSystemTheme : subscribeToNoop,
    shouldResolveSystemTheme ? getSystemTheme : getDefaultTheme,
    getDefaultTheme
  );

  return resolveTheme(themePreference, systemTheme);
}
