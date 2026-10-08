import { getBaseUrl } from '@/common/adapter/httpBridge';
import type { WallpaperId } from '@/common/types/ids';

export function wallpaperDisplayUrl(wallpaperId: WallpaperId, version?: string | number): string {
  const query = version != null ? `?v=${encodeURIComponent(String(version))}` : '';
  return `${getBaseUrl()}/api/appearance/wallpapers/${wallpaperId}/display${query}`;
}

export function wallpaperThumbUrl(wallpaperId: WallpaperId, version?: string | number): string {
  const query = version != null ? `?v=${encodeURIComponent(String(version))}` : '';
  return `${getBaseUrl()}/api/appearance/wallpapers/${wallpaperId}/thumb${query}`;
}

export function wallpaperOriginalUrl(wallpaperId: WallpaperId, version?: string | number): string {
  const query = version != null ? `?v=${encodeURIComponent(String(version))}` : '';
  return `${getBaseUrl()}/api/appearance/wallpapers/${wallpaperId}/original${query}`;
}
