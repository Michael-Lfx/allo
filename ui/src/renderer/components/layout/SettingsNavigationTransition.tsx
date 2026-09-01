import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import LoadingState from '@/renderer/components/beautifulUi/loadingState/LoadingState';

type PendingTransition = {
  target: string;
  token: number;
};

type SettingsNavigationTransitionContextValue = {
  isPending: boolean;
  pendingTarget: string | null;
  navigateWithSettingsTransition: (target: string, navigate: () => void) => void;
  markSettingsNavigationReady: () => void;
};

const noop = () => undefined;

const defaultContext: SettingsNavigationTransitionContextValue = {
  isPending: false,
  pendingTarget: null,
  navigateWithSettingsTransition: (_target, navigate) => navigate(),
  markSettingsNavigationReady: noop,
};

const SettingsNavigationTransitionContext = React.createContext(defaultContext);

const SETTINGS_TRANSITION_BACKSTOP_MS = 4_000;

const locationKey = (pathname: string, search: string, hash: string): string =>
  `${pathname}${search}${hash}` || '/';

const normalizeLocation = (value: string): string => {
  if (!value) return '/';
  const [path, suffix = ''] = value.split(/(?=[?#])/u, 2);
  if (path.length > 1 && path.endsWith('/')) return `${path.slice(0, -1)}${suffix}`;
  return value;
};

export const SettingsNavigationTransitionProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const location = useLocation();
  const currentLocation = locationKey(location.pathname, location.search, location.hash);
  const [pending, setPending] = useState<PendingTransition | null>(null);
  const pendingRef = useRef<PendingTransition | null>(null);
  const tokenRef = useRef(0);
  const frameRef = useRef<number | null>(null);
  const timeoutRef = useRef<number | null>(null);
  const previousLocationRef = useRef(currentLocation);

  const cancelScheduledNavigation = useCallback(() => {
    if (frameRef.current === null || typeof window === 'undefined') return;
    window.cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
  }, []);

  const clearPending = useCallback(
    (token?: number) => {
      const current = pendingRef.current;
      if (!current || (token !== undefined && current.token !== token)) return;

      pendingRef.current = null;
      if (timeoutRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(timeoutRef.current);
      }
      timeoutRef.current = null;
      setPending(null);
    },
    []
  );

  const navigateWithSettingsTransition = useCallback(
    (target: string, navigate: () => void) => {
      if (normalizeLocation(target) === normalizeLocation(currentLocation)) {
        cancelScheduledNavigation();
        clearPending();
        navigate();
        return;
      }

      cancelScheduledNavigation();
      clearPending();

      const next: PendingTransition = {
        target,
        token: tokenRef.current + 1,
      };
      tokenRef.current = next.token;
      pendingRef.current = next;
      setPending(next);

      if (typeof window === 'undefined') {
        try {
          navigate();
        } finally {
          clearPending(next.token);
        }
        return;
      }

      timeoutRef.current = window.setTimeout(() => clearPending(next.token), SETTINGS_TRANSITION_BACKSTOP_MS);
      frameRef.current = window.requestAnimationFrame(() => {
        frameRef.current = null;
        if (pendingRef.current?.token !== next.token) return;
        try {
          navigate();
        } catch (error) {
          clearPending(next.token);
          throw error;
        }
      });
    },
    [cancelScheduledNavigation, clearPending, currentLocation]
  );

  const markSettingsNavigationReady = useCallback(() => {
    const current = pendingRef.current;
    if (!current) return;
    if (normalizeLocation(currentLocation) !== normalizeLocation(current.target)) return;
    clearPending(current.token);
  }, [clearPending, currentLocation]);

  useLayoutEffect(() => {
    const previousLocation = previousLocationRef.current;
    previousLocationRef.current = currentLocation;
    if (previousLocation === currentLocation) return;

    cancelScheduledNavigation();
    const current = pendingRef.current;
    if (current && normalizeLocation(current.target) !== normalizeLocation(currentLocation)) {
      clearPending(current.token);
    }
  }, [cancelScheduledNavigation, clearPending, currentLocation]);

  useEffect(
    () => () => {
      cancelScheduledNavigation();
      if (timeoutRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(timeoutRef.current);
      }
    },
    [cancelScheduledNavigation]
  );

  const value = useMemo<SettingsNavigationTransitionContextValue>(
    () => ({
      isPending: pending !== null,
      pendingTarget: pending?.target ?? null,
      navigateWithSettingsTransition,
      markSettingsNavigationReady,
    }),
    [markSettingsNavigationReady, navigateWithSettingsTransition, pending]
  );

  return (
    <SettingsNavigationTransitionContext.Provider value={value}>
      {children}
    </SettingsNavigationTransitionContext.Provider>
  );
};

export const useSettingsNavigationTransition = (): SettingsNavigationTransitionContextValue =>
  React.useContext(SettingsNavigationTransitionContext);

/**
 * A stable, non-blocking loading layer for the right side of the settings
 * workspace. It is mounted by Layout so it survives route component
 * unmounts and can paint before a heavy settings page commits.
 */
export const SettingsNavigationLoadingOverlay: React.FC = () => {
  const { isPending } = useSettingsNavigationTransition();
  const { t } = useTranslation();
  if (!isPending) return null;

  return (
    <div
      className='pointer-events-none absolute inset-0 z-20 flex items-center justify-center bg-base flowy-crossfade'
      data-testid='settings-navigation-loading'
      aria-busy='true'
    >
      <LoadingState variant='drive' label={t('common.loading', { defaultValue: 'Loading...' })} />
    </div>
  );
};

export default SettingsNavigationTransitionContext;
