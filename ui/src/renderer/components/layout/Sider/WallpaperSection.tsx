import React, { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Slider, Switch } from '@arco-design/web-react';
import { Plus, Delete } from '@icon-park/react';
import classNames from 'classnames';
import { useWallpaper } from '@renderer/hooks/ui/useWallpaper';
import { wallpaperThumbUrl } from '@renderer/utils/theme/wallpaperUrls';
import { AppMessage as Message } from '@/renderer/components/notifications';

const WallpaperSection: React.FC = () => {
  const { t } = useTranslation();
  const fileRef = useRef<HTMLInputElement>(null);
  const { prefs, library, analysis, scene, setPrefs, upload, remove, contrastRatio, busy, activeId } =
    useWallpaper();

  const dimValue = prefs.dim === 'auto' ? Math.round((analysis?.recommendedDim ?? 0.32) * 100) : Math.round(prefs.dim * 100);
  const blurValue = prefs.blur === 'auto' ? Math.round(analysis?.recommendedBlurPx ?? 0) : Math.round(prefs.blur);
  const previewSrc =
    scene.kind === 'video' ? scene.posterUrl || scene.url : scene.kind === 'image' ? scene.url : undefined;

  const onUpload = async (file: File | undefined) => {
    if (!file) return;
    try {
      await upload(file);
      Message.success(t('settings.wallpaper.uploaded'));
    } catch (error) {
      console.error(error);
      Message.error(t('settings.wallpaper.uploadFailed'));
    }
  };

  const applyLibraryItem = (wallpaperId: (typeof library)[number]['wallpaperId'], event?: React.SyntheticEvent) => {
    event?.preventDefault();
    event?.stopPropagation();
    void setPrefs({
      enabled: true,
      kind: 'library',
      id: wallpaperId,
      lightId: null,
      darkId: null,
    });
  };

  const onToggle = (enabled: boolean) => {
    if (!enabled) {
      void setPrefs({ enabled: false });
      return;
    }
    const fallback =
      (prefs.id && library.some((item) => item.wallpaperId === prefs.id) ? prefs.id : library[0]?.wallpaperId) ?? null;
    if (!fallback) {
      fileRef.current?.click();
      return;
    }
    applyLibraryItem(fallback);
  };

  return (
    <div className='flex flex-col gap-8px min-w-0'>
      <div className='flex items-center justify-between gap-8px'>
        <div className='text-11px font-500 text-t-tertiary'>{t('settings.wallpaper.title')}</div>
        <Switch size='small' checked={prefs.enabled} onChange={onToggle} />
      </div>

      <div className='flex gap-6px overflow-x-auto min-w-0 pb-2px items-center'>
        {library.map((item) => {
          const active = prefs.enabled && activeId === item.wallpaperId;
          return (
            <div key={item.wallpaperId} className='relative shrink-0'>
              <button
                type='button'
                title={item.name}
                onPointerDown={(event) => applyLibraryItem(item.wallpaperId, event)}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                }}
                className={classNames(
                  'size-36px rd-6px border-solid cursor-pointer overflow-hidden p-0',
                  active ? 'border-2 border-primary-6' : 'border border-border-2'
                )}
              >
                <img
                  src={wallpaperThumbUrl(item.wallpaperId, item.createdAt)}
                  alt=''
                  className='size-full object-cover block'
                />
              </button>
              <button
                type='button'
                aria-label={t('common.delete')}
                onPointerDown={(event) => event.stopPropagation()}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  void remove(item.wallpaperId);
                }}
                className='absolute -top-4px -right-4px size-14px rd-full border-none bg-fill-3 text-t-secondary cursor-pointer flex items-center justify-center'
              >
                <Delete theme='outline' size='10' fill='currentColor' />
              </button>
            </div>
          );
        })}
        <button
          type='button'
          onClick={() => fileRef.current?.click()}
          className='shrink-0 size-36px rd-6px border border-dashed border-border-2 bg-transparent text-t-tertiary cursor-pointer flex items-center justify-center'
          title={t('settings.wallpaper.upload')}
        >
          <Plus theme='outline' size='14' fill='currentColor' />
        </button>
        <input
          ref={fileRef}
          type='file'
          accept='image/jpeg,image/png,image/webp,image/gif,video/mp4,video/webm'
          className='hidden'
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = '';
            void onUpload(file);
          }}
        />
      </div>
      {library.length === 0 ? (
        <div className='text-11px text-t-tertiary'>{t('settings.wallpaper.empty')}</div>
      ) : null}

      <div
        className='rd-8px border border-solid border-border-2 overflow-hidden h-72px relative'
        style={{
          background: analysis ? analysis.seedHex : 'var(--color-fill-2)',
        }}
      >
        {previewSrc ? (
          <img src={previewSrc} alt='' className='absolute inset-0 w-full h-full object-cover block' />
        ) : null}
        {analysis ? (
          <div
            className='absolute inset-0'
            style={{
              background: `rgba(0,0,0,${prefs.dim === 'auto' ? analysis.recommendedDim : prefs.dim})`,
            }}
          />
        ) : null}
        <div className='absolute left-8px top-10px w-22px h-52px rd-4px bg-[color-mix(in_srgb,var(--color-bg-2)_72%,transparent)]' />
        <div className='absolute right-10px top-14px flex flex-col gap-6px items-end'>
          <div className='h-16px w-92px rd-8px bg-[color-mix(in_srgb,var(--message-user-bg,rgb(var(--primary-6)))_78%,transparent)]' />
          <div className='h-16px w-72px rd-8px bg-[color-mix(in_srgb,var(--color-bg-3)_78%,transparent)]' />
          <div className='h-12px w-110px rd-6px bg-[color-mix(in_srgb,var(--color-bg-1)_80%,transparent)]' />
        </div>
      </div>

      {prefs.enabled && (busy || contrastRatio < 4.5) ? (
        <div className='text-11px text-warning'>
          {busy ? t('settings.wallpaper.busyHint') : t('settings.wallpaper.contrastHint')}
        </div>
      ) : null}

      <div className='flex items-center gap-8px'>
        <span className='text-11px text-t-tertiary w-36px shrink-0'>{t('settings.wallpaper.dim')}</span>
        <Slider
          className='flex-1 min-w-0'
          min={0}
          max={80}
          value={dimValue}
          onChange={(value) => {
            const next = Array.isArray(value) ? value[0] : value;
            void setPrefs({ dim: next / 100 });
          }}
        />
        <button
          type='button'
          className='text-11px text-t-secondary border-none bg-transparent cursor-pointer shrink-0'
          onClick={() => void setPrefs({ dim: 'auto' })}
        >
          {t('settings.wallpaper.auto')}
        </button>
      </div>

      <div className='flex items-center gap-8px'>
        <span className='text-11px text-t-tertiary w-36px shrink-0'>{t('settings.wallpaper.blur')}</span>
        <Slider
          className='flex-1 min-w-0'
          min={0}
          max={24}
          value={blurValue}
          onChange={(value) => {
            const next = Array.isArray(value) ? value[0] : value;
            void setPrefs({ blur: next });
          }}
        />
        <button
          type='button'
          className='text-11px text-t-secondary border-none bg-transparent cursor-pointer shrink-0'
          onClick={() => void setPrefs({ blur: 'auto' })}
        >
          {t('settings.wallpaper.auto')}
        </button>
      </div>

      <label className='flex items-center justify-between gap-8px text-11px text-t-secondary'>
        <span>{t('settings.wallpaper.harmonize')}</span>
        <Switch
          size='small'
          checked={prefs.harmonizeAccent}
          onChange={(harmonizeAccent) => void setPrefs({ harmonizeAccent })}
        />
      </label>
      <label className='flex items-center justify-between gap-8px text-11px text-t-secondary'>
        <span>{t('settings.wallpaper.motion')}</span>
        <Switch
          size='small'
          checked={prefs.videoEnabled && prefs.reducedMotionPolicy === 'play'}
          onChange={(on) =>
            void setPrefs({ videoEnabled: on, reducedMotionPolicy: on ? 'play' : 'freeze' })
          }
        />
      </label>
    </div>
  );
};

export default WallpaperSection;
