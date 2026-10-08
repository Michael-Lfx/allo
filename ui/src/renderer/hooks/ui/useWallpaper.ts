import { ipcBridge } from '@/common';
import { configService } from '@/common/config/configService';
import {
  DEFAULT_WALLPAPER_PREFS,
  type WallpaperAnalysis,
  type WallpaperMeta,
  type WallpaperPrefs,
} from '@/common/types/appearance';
import type { WallpaperId } from '@/common/types/ids';
import { useThemeContext } from '@renderer/hooks/context/ThemeContext';
import { applyWallpaperTokens } from '@renderer/utils/theme/applyWallpaperTokens';
import { broadcastWallpaperSync } from '@renderer/utils/theme/themeBroadcast';
import { normalizeWallpaperPrefs, resolvedWallpaperId } from '@renderer/utils/theme/wallpaperPrefs';
import {
  plateContrastRatio,
  resolveWallpaperDim,
} from '@renderer/utils/theme/wallpaperTokens';
import { wallpaperDisplayUrl, wallpaperOriginalUrl, wallpaperThumbUrl } from '@renderer/utils/theme/wallpaperUrls';
import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';

export interface WallpaperSceneSource {
  kind: 'image' | 'video' | 'none';
  url?: string;
  posterUrl?: string;
}

export interface UseWallpaperResult {
  prefs: WallpaperPrefs;
  library: WallpaperMeta[];
  analysis: WallpaperAnalysis | null;
  scene: WallpaperSceneSource;
  activeId: string | null;
  contrastRatio: number;
  busy: boolean;
  setPrefs: (patch: Partial<WallpaperPrefs>) => Promise<void>;
  upload: (file: File) => Promise<WallpaperMeta>;
  remove: (wallpaperId: WallpaperId) => Promise<void>;
  reloadLibrary: () => Promise<void>;
}

const WallpaperContext = createContext<UseWallpaperResult | null>(null);

const persistPrefs = async (prefs: WallpaperPrefs): Promise<void> => {
  await configService.set('appearance.wallpaper', prefs);
};

const upsertWallpaper = (items: WallpaperMeta[], next: WallpaperMeta): WallpaperMeta[] => [
  next,
  ...items.filter((item) => item.wallpaperId !== next.wallpaperId),
];

const useWallpaperController = (): UseWallpaperResult => {
  const { theme } = useThemeContext();
  const [prefs, setPrefsState] = useState<WallpaperPrefs>(() =>
    normalizeWallpaperPrefs(configService.get('appearance.wallpaper'))
  );
  const [library, setLibrary] = useState<WallpaperMeta[]>([]);
  const fetchedIdRef = useRef<string | null>(null);

  const reloadLibrary = useCallback(async () => {
    try {
      const items = await ipcBridge.appearance.listWallpapers.invoke();
      setLibrary(items);
    } catch (error) {
      console.error('Failed to load wallpaper library:', error);
    }
  }, []);

  useEffect(() => {
    void reloadLibrary();
    const unsubscribe = configService.subscribe('appearance.wallpaper', (value) => {
      setPrefsState(normalizeWallpaperPrefs(value as WallpaperPrefs | undefined));
    });
    return unsubscribe;
  }, [reloadLibrary]);

  const setPrefs = useCallback(async (patch: Partial<WallpaperPrefs>) => {
    let next: WallpaperPrefs | undefined;
    setPrefsState((current) => {
      next = normalizeWallpaperPrefs({ ...current, ...patch });
      return next;
    });
    if (next) await persistPrefs(next);
  }, []);

  const libraryNewestFirst = useMemo(
    () => library.toSorted((left, right) => right.createdAt - left.createdAt),
    [library]
  );

  const activeId = resolvedWallpaperId(prefs, theme);
  const libraryItem = library.find((item) => item.wallpaperId === activeId);

  useEffect(() => {
    if (!prefs.enabled || !activeId || libraryItem) return;
    if (fetchedIdRef.current === activeId) return;
    fetchedIdRef.current = activeId;
    void ipcBridge.appearance.getWallpaper
      .invoke({ wallpaper_id: activeId as WallpaperId })
      .then((item) => {
        setLibrary((current) => upsertWallpaper(current, item));
      })
      .catch((error) => {
        console.error('Failed to load active wallpaper:', error);
      });
  }, [prefs.enabled, activeId, libraryItem]);

  const analysis = useMemo<WallpaperAnalysis | null>(() => {
    if (!prefs.enabled || !activeId) return null;
    return libraryItem?.analysis ?? null;
  }, [prefs.enabled, activeId, libraryItem]);

  const scene = useMemo<WallpaperSceneSource>(() => {
    if (!prefs.enabled || !activeId) return { kind: 'none' };
    if (!libraryItem) return { kind: 'none' };
    const version = libraryItem.createdAt;
    if (libraryItem.mediaKind === 'video') {
      return {
        kind: 'video',
        url: wallpaperOriginalUrl(libraryItem.wallpaperId, version),
        posterUrl: wallpaperThumbUrl(libraryItem.wallpaperId, version),
      };
    }
    return {
      kind: 'image',
      url: wallpaperDisplayUrl(libraryItem.wallpaperId, version),
      posterUrl:
        libraryItem.mediaKind === 'animated'
          ? wallpaperThumbUrl(libraryItem.wallpaperId, version)
          : undefined,
    };
  }, [prefs.enabled, activeId, libraryItem]);

  useEffect(() => {
    const css = applyWallpaperTokens({
      enabled: prefs.enabled,
      prefs,
      analysis,
      scheme: theme,
    });
    broadcastWallpaperSync({
      wallpaperTokens: css,
      wallpaperEnabled: Boolean(prefs.enabled && analysis),
    });
  }, [prefs, analysis, theme]);

  const upload = useCallback(
    async (file: File) => {
      const { uploadFileViaHttp } = await import('@renderer/services/FileService');
      const source_path = await uploadFileViaHttp(file, undefined, undefined, file.name);
      const created = await ipcBridge.appearance.createWallpaper.invoke({
        source_path,
        name: file.name.replace(/\.[^.]+$/, ''),
      });
      setLibrary((current) => upsertWallpaper(current, created));
      const motion = created.mediaKind === 'video' || created.mediaKind === 'animated';
      await setPrefs({
        enabled: true,
        kind: 'library',
        id: created.wallpaperId,
        lightId: null,
        darkId: null,
        videoEnabled: motion ? true : prefs.videoEnabled,
      });
      void reloadLibrary();
      return created;
    },
    [prefs.videoEnabled, reloadLibrary, setPrefs]
  );

  const remove = useCallback(
    async (wallpaperId: WallpaperId) => {
      await ipcBridge.appearance.deleteWallpaper.invoke({ wallpaper_id: wallpaperId });
      const remaining = libraryNewestFirst.filter((item) => item.wallpaperId !== wallpaperId);
      setLibrary(remaining);
      const nextId = remaining[0]?.wallpaperId ?? null;
      const clearingActive = prefs.id === wallpaperId || prefs.lightId === wallpaperId || prefs.darkId === wallpaperId;
      if (clearingActive) {
        await setPrefs({
          kind: 'library',
          id: prefs.id === wallpaperId ? nextId : prefs.id,
          lightId: prefs.lightId === wallpaperId ? nextId : prefs.lightId,
          darkId: prefs.darkId === wallpaperId ? nextId : prefs.darkId,
          enabled: Boolean(nextId) && prefs.enabled,
        });
      }
    },
    [libraryNewestFirst, prefs, setPrefs]
  );

  const contrastRatio = useMemo(
    () =>
      plateContrastRatio(
        analysis,
        resolveWallpaperDim(prefs, analysis),
        analysis?.recommendedPlateAlpha ?? 0.72,
        theme
      ),
    [analysis, prefs, theme]
  );

  return {
    prefs,
    library: libraryNewestFirst,
    analysis,
    scene,
    activeId,
    contrastRatio,
    busy: Boolean(analysis?.busy),
    setPrefs,
    upload,
    remove,
    reloadLibrary,
  };
};

export const WallpaperProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const value = useWallpaperController();
  return React.createElement(WallpaperContext.Provider, { value }, children);
};

export const useWallpaper = (): UseWallpaperResult => {
  const value = useContext(WallpaperContext);
  if (!value) {
    throw new Error('useWallpaper must be used within WallpaperProvider');
  }
  return value;
};

export { DEFAULT_WALLPAPER_PREFS };
