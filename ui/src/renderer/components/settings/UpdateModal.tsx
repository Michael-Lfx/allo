
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Progress } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { CheckOne, Download, Refresh, CloseOne } from '@icon-park/react';
import { ipcBridge } from '@/common';
import NomiModal from '@/renderer/components/base/NomiModal';
import MarkdownView from '@/renderer/components/Markdown';
import type {
  UpdateDownloadProgressEvent,
  UpdateReleaseInfo,
  AutoUpdateInstallPhase,
  AutoUpdateStatus,
} from '@/common/update/updateTypes';
import { useTranslation } from 'react-i18next';
import { getUpdateErrorMessageKey } from './updateErrorMessage';
import { deriveUpdateStatus, shouldApplyDownloadEvent } from './deriveUpdateStatus';
import { reportNoUpdateAvailable, reportUpdateAvailable } from '@renderer/hooks/system/useUpdateAvailability';
import { isDesktopShell } from '@/renderer/utils/platform';
import type { TauriUpdatePackageState } from '@/common/adapter/tauriShell';

type UpdateStatus =
  | 'checking'
  | 'upToDate'
  | 'available'
  | 'downloading'
  | 'downloaded'
  | 'installing'
  | 'success'
  | 'error';

type UpdateInfo = UpdateReleaseInfo;

type UpdatePresentation = 'compact' | 'detail';

const formatVersion = (version?: string | null) => {
  if (!version) return '-';
  return version.startsWith('v') ? version : `v${version}`;
};

const BAIDU_RELEASE_MIRROR_URL = 'https://pan.baidu.com/s/5GPonoJNrwJ7GciBSDgXLaA';

const COMPACT_HOST_CLASS =
  'update-compact-card-host pointer-events-none fixed bottom-24px right-24px z-10020 w-[min(360px,calc(100vw-32px))]';
const COMPACT_CARD_CLASS =
  'update-compact-card pointer-events-auto box-border w-full rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] p-14px text-t-primary shadow-[0_14px_36px_rgba(15,23,42,0.16),0_3px_10px_rgba(15,23,42,0.08)]';
const COMPACT_ACTION_CLASS = 'update-compact-card__action !h-28px !w-full !min-w-0 !justify-center !rounded-7px !text-12px !font-600';

const UpdateModal: React.FC = () => {
  const { t } = useTranslation();
  /** Bundled Tauri shell — in-app OTA via ModelScope + tauri-plugin-updater only. */
  const isNativeUpdater = isDesktopShell();
  const [visible, setVisible] = useState(false);
  const [presentation, setPresentation] = useState<UpdatePresentation>('compact');
  const [status, setStatus] = useState<UpdateStatus>('checking');
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string>('');
  const [downloadId, setDownloadId] = useState<string | null>(null);
  const [progress, setProgress] = useState({ percent: 0, speed: '', total: 0, transferred: 0 });
  const [installPhase, setInstallPhase] = useState<AutoUpdateInstallPhase>('preparing');
  const [errorMsg, setErrorMsg] = useState('');
  const [downloadPath, setDownloadPath] = useState('');
  // Whether electron-updater auto-update is available (determined automatically, not user-controllable)
  const [autoUpdateAvailable, setAutoUpdateAvailable] = useState(false);
  const [autoUpdateInfo, setAutoUpdateInfo] = useState<{ version: string; releaseNotes?: string } | null>(null);
  const installRequestedRef = useRef(false);
  // A download already in flight. `startDownload` had no re-entrancy guard, so a
  // second trigger started a second flow whose independent byte counter fought
  // the first one over the single progress bar.
  const downloadRequestedRef = useRef(false);
  // Version the in-flight download belongs to, so a progress frame from a
  // superseded flow cannot repaint the bar.
  const downloadVersionRef = useRef<string | null>(null);

  const resetState = () => {
    setPresentation('compact');
    setStatus('checking');
    setUpdateInfo(null);
    setCurrentVersion('');
    setDownloadId(null);
    setProgress({ percent: 0, speed: '', total: 0, transferred: 0 });
    setInstallPhase('preparing');
    installRequestedRef.current = false;
    downloadRequestedRef.current = false;
    downloadVersionRef.current = null;
    setErrorMsg('');
    setDownloadPath('');
    setAutoUpdateAvailable(false);
    setAutoUpdateInfo(null);
  };

  const includePrerelease = useMemo(() => localStorage.getItem('update.includePrerelease') === 'true', [visible]);
  const hasCompatibleManualAsset = Boolean(updateInfo?.recommendedAsset);

  const openBaiduReleaseMirror = () => {
    void ipcBridge.shell.openExternal.invoke(BAIDU_RELEASE_MIRROR_URL).catch((error) => {
      console.error('Failed to open Baidu release mirror:', error);
    });
  };

  const checkForUpdates = async () => {
    // Every check result belongs to the compact surface. In particular, retrying
    // from expanded error details must not revive the removed legacy checking
    // modal while the request is in flight.
    setPresentation('compact');
    setStatus('checking');
    try {
      if (isNativeUpdater) {
        const res = await ipcBridge.autoUpdate.check.invoke({ includePrerelease });
        if (!res?.success) {
          throw new Error(res.msg || t('update.nativeCheckFailed'));
        }

        const detail = await ipcBridge.update.check.invoke({ includePrerelease });
        setCurrentVersion(detail.data?.currentVersion || '');

        if (res.data?.updateInfo) {
          setAutoUpdateAvailable(true);
          setAutoUpdateInfo({
            version: res.data.updateInfo.version,
            releaseNotes: res.data.updateInfo.releaseNotes,
          });
          if (detail.data?.latest) {
            setUpdateInfo(detail.data.latest);
          }
          // The native slot is the only thing that knows whether bytes are
          // already retained or a download is still running; derive from it so a
          // re-check can happen at any time and always lands on the truth.
          const derived = deriveUpdateStatus({
            availableVersion: res.data.updateInfo.version,
            retainedVersion: res.data.retainedVersion ?? null,
            slotState: res.data.packageState ?? null,
            slotVersion: res.data.packageVersion ?? null,
          });
          if (derived === 'downloading') {
            // Re-attach to the running download rather than re-arming Download.
            downloadRequestedRef.current = true;
            downloadVersionRef.current = res.data.packageVersion ?? null;
          }
          setStatus(derived);
          return;
        }

        if (!detail?.success) {
          throw new Error(detail.msg || t('update.nativeCheckFailed'));
        }

        if (detail.data && !detail.data.updateAvailable) {
          setStatus('upToDate');
          return;
        }

        throw new Error(t('update.nativeCheckFailed'));
      }

      // WebUI / legacy manual download path (GitHub assets + optional mirrors).
      let autoUpdateOk = false;
      let retainedVersion: string | null = null;
      let packageState: TauriUpdatePackageState | null = null;
      let packageVersion: string | null = null;
      // Captured locally: setAutoUpdateInfo below only lands on the NEXT render,
      // so reading that state back in this same pass would see a stale version.
      let autoUpdateVersion = '';
      try {
        const res = await ipcBridge.autoUpdate.check.invoke({ includePrerelease });
        retainedVersion = res?.data?.retainedVersion ?? null;
        packageState = res?.data?.packageState ?? null;
        packageVersion = res?.data?.packageVersion ?? null;
        if (res?.success && res.data?.updateInfo) {
          autoUpdateOk = true;
          autoUpdateVersion = res.data.updateInfo.version;
          reportUpdateAvailable(res.data.updateInfo.version);
          setAutoUpdateInfo({
            version: res.data.updateInfo.version,
            releaseNotes: res.data.updateInfo.releaseNotes,
          });
        } else if (res?.msg) {
          console.warn('Auto-update check failed, using manual mode:', res.msg);
        }
      } catch (err) {
        console.warn('Auto-update check error, using manual mode:', err);
      }
      setAutoUpdateAvailable(autoUpdateOk);

      // Always run manual check for version info and release notes
      const res = await ipcBridge.update.check.invoke({ includePrerelease });
      if (!res?.success) {
        throw new Error(res?.msg || t('update.checkFailed'));
      }
      setCurrentVersion(res.data?.currentVersion || '');

      if (autoUpdateOk) {
        // Auto-update available — use manual check data for display only
        if (res.data?.latest) {
          setUpdateInfo(res.data.latest);
        }
        // The native slot is the only thing that knows whether bytes are already
        // retained or a download is still running; derive from it so a re-check
        // can happen at any time and always lands on the truth.
        const availableVersion = res.data?.latest?.version || autoUpdateVersion;
        const derived = deriveUpdateStatus({
          availableVersion,
          retainedVersion,
          slotState: packageState,
          slotVersion: packageVersion,
        });
        if (derived === 'downloading') {
          // Re-attach to the running download rather than re-arming Download.
          downloadRequestedRef.current = true;
          downloadVersionRef.current = packageVersion;
        }
        setStatus(derived);
        return;
      }

      // Manual mode
      if (res.data?.updateAvailable && res.data.latest) {
        reportUpdateAvailable(res.data.latest.version);
        setUpdateInfo(res.data.latest);
        if (!res.data.latest.recommendedAsset) {
          setErrorMsg(t('update.noCompatibleAssetManual'));
        }
        setStatus('available');
        return;
      }

      setUpdateInfo(res.data?.latest || null);
      reportNoUpdateAvailable();
      setStatus('upToDate');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('Update check failed:', err);
      if (isNativeUpdater) {
        setErrorMsg(t('update.nativeCheckFailed'));
      } else {
        const errorMessageKey = getUpdateErrorMessageKey(msg);
        setErrorMsg(errorMessageKey === 'update.releaseFeedUnavailable' ? t(errorMessageKey) : msg || t(errorMessageKey));
      }
      setStatus('error');
    }
  };

  const startDownload = async () => {
    if (downloadRequestedRef.current) return;
    if (!updateInfo && !autoUpdateAvailable) return;
    downloadRequestedRef.current = true;
    downloadVersionRef.current = updateInfo?.version || autoUpdateInfo?.version || null;
    // The compact card owns progress presentation. This does not alter the
    // existing download path; it only collapses the optional detail dialog.
    setPresentation('compact');
    setStatus('downloading');
    setProgress({ percent: 0, speed: '', total: 0, transferred: 0 });
    try {
      if (isNativeUpdater) {
        if (!autoUpdateAvailable) {
          throw new Error(t('update.noCompatibleAssetManual'));
        }
        const res = await ipcBridge.autoUpdate.download.invoke();
        if (!res?.success) {
          throw new Error(res?.msg || t('update.downloadStartFailed'));
        }
        // The native download is complete once this resolves (the status emitter
        // has already moved the UI to 'downloaded'), so the guard can be released
        // for a genuine future retry.
        downloadRequestedRef.current = false;
        return;
      }

      // Prefer the manual path so the URL is the CDN-rewritten asset.url.
      // Fall back to electron-updater (GitHub) only when the GitHub API manual check failed
      // but the yml-based auto-update check succeeded — a rare edge case.
      // 优先走手动路径（URL 是重写后的 CDN 地址）。仅当 GitHub API 失败但 electron-updater 检查成功时，
      // 回退到 electron-updater 的下载（走 GitHub），保证用户能升级。
      if (updateInfo?.recommendedAsset) {
        const asset = updateInfo.recommendedAsset;
        const res = await ipcBridge.update.download.invoke({
          url: asset.url,
          fallbackUrl: asset.fallbackUrl,
          file_name: asset.name,
        });
        if (!res?.success || !res.data) {
          throw new Error(res?.msg || t('update.downloadStartFailed'));
        }
        setDownloadId(res.data.downloadId);
        setDownloadPath(res.data.file_path);
        return;
      }

      if (autoUpdateAvailable) {
        const res = await ipcBridge.autoUpdate.download.invoke();
        if (!res?.success) {
          throw new Error(res?.msg || t('update.downloadStartFailed'));
        }
        // The native download is complete once this resolves (the status emitter
        // has already moved the UI to 'downloaded'), so the guard can be
        // released for a genuine future retry.
        downloadRequestedRef.current = false;
        return;
      }

      throw new Error(t('update.noCompatibleAssetManual'));
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('Download failed:', err);
      // A failed download must be retryable; only a LIVE download holds the guard.
      downloadRequestedRef.current = false;
      downloadVersionRef.current = null;
      setErrorMsg(msg);
      setStatus('error');
    }
  };

  const quitAndInstall = async () => {
    if (installRequestedRef.current) return;
    installRequestedRef.current = true;
    setInstallPhase('preparing');
    setProgress({ percent: 0, speed: '', total: 0, transferred: 0 });
    setErrorMsg('');
    setStatus('installing');
    try {
      await ipcBridge.autoUpdate.quitAndInstall.invoke();
    } catch (err: unknown) {
      installRequestedRef.current = false;
      const msg = err instanceof Error ? err.message : String(err);
      console.error('Install failed:', err);
      const messageKey = getUpdateErrorMessageKey(msg);
      Message.error(t(messageKey));
      if (messageKey === 'update.packageNoLongerReady') {
        // The native side no longer holds this package, so the 'downloaded'
        // screen would offer an Install button that can only fail again — and its
        // only other affordance is the manual mirror, not the re-download the
        // message asks for. Re-check instead: the status is then derived from the
        // slot and the user lands on a screen that can actually act.
        void checkForUpdates();
        return;
      }
      setStatus('downloaded');
    }
  };

  const formatSpeed = (bytesPerSecond: number) => {
    if (bytesPerSecond > 1024 * 1024) {
      return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
    }
    return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
  };

  const formatSize = (bytes: number) => {
    if (bytes > 1024 * 1024) {
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }
    return `${(bytes / 1024).toFixed(1)} KB`;
  };

  const handleOpenUpdateModal = () => {
    setVisible(true);
    if (installRequestedRef.current) return;
    // Always re-check, even while a download is running. Skipping the reset here
    // instead looked safer but disabled the ONLY recovery path: a download whose
    // invoke never settles (sleep / network switch) would leave the guard set and
    // the modal frozen for the rest of the session. checkForUpdates re-derives
    // 'downloading' from the native slot, so a live download is re-attached
    // rather than hidden behind a re-armed Download button.
    resetState();
    void checkForUpdates();
  };

  useEffect(() => {
    const removeOpenListener = ipcBridge.update.open.on(handleOpenUpdateModal);
    window.addEventListener('nomifun-open-update-modal', handleOpenUpdateModal);

    return () => {
      removeOpenListener();
      window.removeEventListener('nomifun-open-update-modal', handleOpenUpdateModal);
    };
  }, []);

  // Listen for auto-update status events (e.g. from startup check)
  useEffect(() => {
    const removeListener = ipcBridge.autoUpdate.status.on((evt: AutoUpdateStatus) => {
      if (!evt) return;
      // Discard every frame from a superseded download flow, terminal ones
      // included: a stale completion used to flip the modal to the Install screen
      // while the live download was still mid-transfer.
      if (
        (evt.status === 'downloading' || evt.status === 'downloaded' || evt.status === 'error') &&
        !shouldApplyDownloadEvent(evt.version, downloadVersionRef.current)
      ) {
        return;
      }

      switch (evt.status) {
        case 'checking':
          break;
        case 'available':
          reportUpdateAvailable(evt.version);
          setAutoUpdateAvailable(true);
          setAutoUpdateInfo({
            version: evt.version || '',
            releaseNotes: evt.releaseNotes,
          });
          setStatus('available');
          setVisible(true);
          break;
        case 'not-available':
          reportNoUpdateAvailable();
          setStatus('upToDate');
          break;
        case 'downloading':
          // Ignore a series from a superseded download: two live flows keep
          // separate byte counters, and letting both write here is what made one
          // bar flip between two unrelated progress readings.
          if (evt.progress) {
            setProgress({
              percent: Math.round(evt.progress.percent),
              speed: formatSpeed(evt.progress.bytesPerSecond),
              total: evt.progress.total,
              transferred: evt.progress.transferred,
            });
          }
          break;
        case 'downloaded':
          downloadRequestedRef.current = false;
          setStatus('downloaded');
          break;
        case 'installing':
          setStatus('installing');
          setInstallPhase(evt.installPhase || 'preparing');
          if (evt.progress) {
            setProgress({
              percent: Math.round(evt.progress.percent),
              speed: formatSpeed(evt.progress.bytesPerSecond),
              total: evt.progress.total,
              transferred: evt.progress.transferred,
            });
          }
          break;
        case 'error':
          setStatus('error');
          setErrorMsg(evt.error || t('update.downloadFailed'));
          break;
      }
    });

    return () => {
      removeListener();
    };
  }, [t]);

  useEffect(() => {
    const removeProgressListener = ipcBridge.update.downloadProgress.on((evt: UpdateDownloadProgressEvent) => {
      if (!evt) return;
      if (!downloadId || evt.downloadId !== downloadId) return;

      setProgress({
        percent: Math.round(evt.percent ?? 0),
        speed: formatSpeed(evt.bytesPerSecond ?? 0),
        total: evt.totalBytes ?? 0,
        transferred: evt.receivedBytes ?? 0,
      });

      if (evt.status === 'completed') {
        downloadRequestedRef.current = false;
        setStatus('success');
        if (evt.file_path) {
          setDownloadPath(evt.file_path);
        }
      } else if (evt.status === 'error' || evt.status === 'cancelled') {
        downloadRequestedRef.current = false;
        setStatus('error');
        setErrorMsg(evt.error || t('update.downloadFailed'));
      }
    });

    return () => {
      removeProgressListener();
    };
  }, [downloadId, t]);

  const handleClose = () => {
    if (installRequestedRef.current) return;
    setVisible(false);
  };

  const showDetails = () => {
    if (status === 'available' || status === 'error') {
      setPresentation('detail');
    }
  };

  const openFile = () => {
    if (!downloadPath) return;
    void ipcBridge.shell.openFile.invoke(downloadPath).catch((error) => {
      console.error('Failed to open file:', error);
    });
  };

  const showInFolder = () => {
    if (!downloadPath) return;
    void ipcBridge.shell.showItemInFolder.invoke(downloadPath).catch((error) => {
      console.error('Failed to show item in folder:', error);
    });
  };

  const renderBaiduManualDownloadButton = (className = '') => {
    if (isNativeUpdater) return null;
    return (
      <Button
        size='small'
        onClick={openBaiduReleaseMirror}
        icon={<Download size='14' />}
        className={`flowy-icon-text-btn !px-16px ${className}`}
      >
        {t('settings.baiduManualDownload')}
      </Button>
    );
  };

  const renderManualDownloadHints = () => {
    if (isNativeUpdater) return null;

    return (
      <div className='mx-24px mt-12px px-12px py-10px rounded-8px border border-solid border-[rgba(var(--primary-6),0.16)] bg-[rgba(var(--primary-6),0.06)] text-12px leading-18px text-t-secondary'>
        <div>
          {t('update.baiduMirrorHint')}{' '}
          <button
            type='button'
            onClick={openBaiduReleaseMirror}
            title={BAIDU_RELEASE_MIRROR_URL}
            className='cursor-pointer border-0 bg-transparent p-0 text-12px leading-18px text-[rgb(var(--primary-6))] underline-offset-2 hover:underline'
          >
            {t('update.baiduMirrorLink')}
          </button>
        </div>
      </div>
    );
  };

  const renderCompactContent = () => {
    const availableVersion = updateInfo?.version || autoUpdateInfo?.version || '';
    const canDismiss = status !== 'installing';
    const title =
      status === 'downloading' || status === 'downloaded' || status === 'installing'
        ? t('update.compactProgressTitle')
        : status === 'error'
          ? t('update.errorTitle')
          : t('update.compactTitle');

    return (
      <section
        className={`${COMPACT_CARD_CLASS} update-compact-card--${status}`}
        role='dialog'
        aria-modal='false'
        aria-label={title}
      >
        <div className='mb-8px flex min-h-24px items-center justify-between gap-12px'>
          <div className='min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-14px font-650 leading-20px'>
            {title}
          </div>
          {canDismiss && (
            <button
              type='button'
              className='inline-flex h-24px w-24px shrink-0 cursor-pointer items-center justify-center rounded-6px border-0 bg-transparent p-0 text-18px leading-none text-t-tertiary hover:bg-[var(--color-fill-2)] hover:text-t-primary'
              onClick={handleClose}
              aria-label={t('common.close')}
            >
              <span aria-hidden='true'>×</span>
            </button>
          )}
        </div>

        {status === 'checking' && (
          <div className='flex min-w-0 items-start gap-10px text-13px leading-19px text-t-secondary' aria-live='polite'>
            <span
              className='mt-1px h-16px w-16px shrink-0 animate-spin rounded-full border-2px border-solid border-[rgba(var(--primary-6),0.2)] border-t-[rgb(var(--primary-6))]'
              aria-hidden='true'
            />
            <span>{t('update.checking')}</span>
          </div>
        )}

        {status === 'upToDate' && (
          <div className='flex min-w-0 items-start gap-10px text-13px leading-19px text-t-secondary' aria-live='polite'>
            <span className='inline-flex h-24px w-24px shrink-0 items-center justify-center rounded-full bg-[rgb(var(--success-6))]/12 text-[rgb(var(--success-6))]'>
              <CheckOne theme='filled' size='15' />
            </span>
            <div>
              <div className='text-13px font-550 leading-19px text-t-primary'>{t('update.upToDateTitle')}</div>
              <div className='overflow-hidden text-ellipsis whitespace-nowrap text-12px leading-18px text-t-tertiary'>
                {t('update.currentVersion', { version: formatVersion(currentVersion) })}
              </div>
            </div>
          </div>
        )}

        {status === 'available' && (
          <>
            <div className='flex min-w-0 items-baseline gap-6px text-13px leading-20px text-t-secondary'>
              <span>{formatVersion(currentVersion)}</span>
              <span className='text-t-tertiary' aria-hidden='true'>
                →
              </span>
              <strong className='font-650 text-t-primary'>{formatVersion(availableVersion)}</strong>
              <button
                type='button'
                className='update-compact-card__detail-link ml-auto shrink-0 cursor-pointer border-0 bg-transparent p-0 text-12px leading-18px text-[rgb(var(--primary-6))] underline-offset-2 hover:underline'
                onClick={showDetails}
              >
                {t('update.viewDetails')}
              </button>
            </div>
            <div className='mt-10px grid grid-cols-2 gap-8px'>
              <Button type='primary' size='mini' onClick={startDownload} className={COMPACT_ACTION_CLASS}>
                {t('update.updateNow')}
              </Button>
              <Button size='mini' onClick={handleClose} className={COMPACT_ACTION_CLASS}>
                {t('update.later')}
              </Button>
            </div>
          </>
        )}

        {status === 'downloading' && (
          <div aria-live='polite'>
            <div className='mb-8px flex min-w-0 items-baseline justify-between gap-12px text-12px leading-18px text-t-primary'>
              <span>{formatVersion(availableVersion || downloadVersionRef.current)}</span>
              <span className='overflow-hidden text-ellipsis whitespace-nowrap text-t-tertiary'>
                {t('update.downloadingTitle')}
              </span>
            </div>
            <Progress
              percent={progress.percent}
              status='normal'
              showText={false}
              strokeWidth={6}
              className='!mb-0 !block'
            />
            <div className='mt-5px flex min-w-0 items-center justify-between gap-12px text-11px leading-16px text-t-tertiary'>
              <span className='overflow-hidden text-ellipsis whitespace-nowrap'>
                {progress.total > 0
                  ? `${formatSize(progress.transferred)} / ${formatSize(progress.total)}`
                  : formatSize(progress.transferred)}
              </span>
              <span className='overflow-hidden text-ellipsis whitespace-nowrap'>
                {progress.speed || `${Math.round(progress.percent)}%`}
              </span>
            </div>
          </div>
        )}

        {status === 'downloaded' && (
          <>
            <div className='flex min-w-0 items-start gap-10px text-13px leading-19px text-t-secondary' aria-live='polite'>
              <span className='inline-flex h-24px w-24px shrink-0 items-center justify-center rounded-full bg-[rgb(var(--success-6))]/12 text-[rgb(var(--success-6))]'>
                <CheckOne theme='filled' size='15' />
              </span>
              <div>
                <div className='text-13px font-550 leading-19px text-t-primary'>{t('update.readyToInstall')}</div>
                <div className='overflow-hidden text-ellipsis whitespace-nowrap text-12px leading-18px text-t-tertiary'>
                  {t('update.readyToInstallDesc')}
                </div>
              </div>
            </div>
            <div className='mt-10px grid grid-cols-2 gap-8px'>
              <Button type='primary' size='mini' onClick={quitAndInstall} className={COMPACT_ACTION_CLASS}>
                {t('update.installNow')}
              </Button>
              <Button size='mini' onClick={handleClose} className={COMPACT_ACTION_CLASS}>
                {t('update.later')}
              </Button>
            </div>
          </>
        )}

        {status === 'installing' && (
          <div className='flex min-w-0 items-start gap-10px text-13px leading-19px text-t-secondary' aria-live='polite'>
            <span
              className='mt-1px h-16px w-16px shrink-0 animate-spin rounded-full border-2px border-solid border-[rgba(var(--primary-6),0.2)] border-t-[rgb(var(--primary-6))]'
              aria-hidden='true'
            />
            <div>
              <div className='text-13px font-550 leading-19px text-t-primary'>
                {installPhase === 'installing' ? t('update.installingTitle') : t('update.preparingInstallTitle')}
              </div>
              <div className='overflow-hidden text-ellipsis whitespace-nowrap text-12px leading-18px text-t-tertiary'>
                {installPhase === 'installing' ? t('update.installingDesc') : t('update.preparingInstallDesc')}
              </div>
            </div>
          </div>
        )}

        {status === 'success' && (
          <>
            <div className='flex min-w-0 items-start gap-10px text-13px leading-19px text-t-secondary' aria-live='polite'>
              <span className='inline-flex h-24px w-24px shrink-0 items-center justify-center rounded-full bg-[rgb(var(--success-6))]/12 text-[rgb(var(--success-6))]'>
                <CheckOne theme='filled' size='15' />
              </span>
              <div className='text-13px font-550 leading-19px text-t-primary'>{t('update.downloadCompleteTitle')}</div>
            </div>
            <div className='mt-10px grid grid-cols-2 gap-8px'>
              <Button size='mini' onClick={showInFolder} className={COMPACT_ACTION_CLASS}>
                {t('update.showInFolder')}
              </Button>
              <Button type='primary' size='mini' onClick={openFile} className={COMPACT_ACTION_CLASS}>
                {t('update.openFile')}
              </Button>
            </div>
          </>
        )}

        {status === 'error' && (
          <>
            <div className='line-clamp-2 text-12px leading-18px text-t-secondary' aria-live='polite'>
              {errorMsg}
            </div>
            <div className='mt-6px'>
              <button
                type='button'
                className='update-compact-card__detail-link mb-8px block cursor-pointer border-0 bg-transparent p-0 text-12px leading-18px text-[rgb(var(--primary-6))] underline-offset-2 hover:underline'
                onClick={showDetails}
              >
                {t('update.viewDetails')}
              </button>
              <div className='grid grid-cols-2 gap-8px'>
                <Button type='primary' size='mini' onClick={checkForUpdates} className={COMPACT_ACTION_CLASS}>
                  {t('common.retry')}
                </Button>
                <Button size='mini' onClick={handleClose} className={COMPACT_ACTION_CLASS}>
                  {t('update.later')}
                </Button>
              </div>
            </div>
          </>
        )}
      </section>
    );
  };

  const renderDetailContent = () => {
    switch (status) {
      case 'available':
        return (
          <div className='flex h-full min-h-0 flex-col'>
            <div className='flex items-center justify-between border-b border-b-solid border-arco-2 bg-fill-1 px-24px py-16px'>
              <div className='flex items-center gap-12px'>
                <div className='flex h-40px w-40px items-center justify-center rounded-10px bg-[rgb(var(--primary-6))]/12'>
                  <Download size='20' fill='rgb(var(--primary-6))' />
                </div>
                <div>
                  <div className='text-15px font-600 text-t-primary'>{t('update.availableTitle')}</div>
                  <div className='mt-2px text-12px text-t-tertiary'>
                    {currentVersion} →{' '}
                    <span className='font-500 text-[rgb(var(--primary-6))]'>
                      {updateInfo?.version || autoUpdateInfo?.version}
                    </span>
                  </div>
                </div>
              </div>
              <div className='flex flex-wrap items-center justify-end gap-8px'>
                <Button type='primary' size='small' onClick={startDownload} className='!px-16px'>
                  {t('update.downloadButton')}
                </Button>
                {renderBaiduManualDownloadButton()}
              </div>
            </div>

            {!isNativeUpdater && !hasCompatibleManualAsset && !autoUpdateAvailable && (
              <div className='mx-24px mt-12px rounded-8px bg-[rgb(var(--warning-6))]/10 px-12px py-10px text-12px text-[rgb(var(--warning-6))]'>
                {t('update.noCompatibleAssetManual')}
              </div>
            )}

            {renderManualDownloadHints()}

            <div
              className='update-modal__release-scroll custom-scrollbar min-h-0 flex-1 overflow-y-auto px-24px py-16px'
              tabIndex={0}
              role='region'
              aria-label={t('update.availableTitle')}
            >
              {updateInfo?.name && <div className='mb-12px text-14px font-500 text-t-primary'>{updateInfo.name}</div>}
              {updateInfo?.body || autoUpdateInfo?.releaseNotes ? (
                <div className='text-13px leading-relaxed text-t-secondary'>
                  <MarkdownView allowHtml>{updateInfo?.body || autoUpdateInfo?.releaseNotes || ''}</MarkdownView>
                </div>
              ) : (
                <div className='text-13px italic text-t-tertiary'>{t('update.noReleaseNotes')}</div>
              )}
            </div>
          </div>
        );

      case 'error':
        return (
          <div className='flex flex-col items-center px-24px py-28px'>
            <div className='mb-12px flex h-48px w-48px items-center justify-center rounded-full bg-[rgb(var(--danger-6))]/12'>
              <CloseOne theme='filled' size='24' fill='rgb(var(--danger-6))' />
            </div>
            <div className='text-15px font-600 text-t-primary'>{t('update.errorTitle')}</div>
            <div className='mt-8px max-w-420px text-center text-12px leading-18px text-t-secondary' aria-live='polite'>
              {errorMsg}
            </div>
            <div className='mt-20px flex flex-wrap justify-center gap-8px'>
              <Button onClick={checkForUpdates} icon={<Refresh size='16' />} className='flowy-icon-text-btn !px-20px'>
                {t('common.retry')}
              </Button>
              {!isNativeUpdater && renderBaiduManualDownloadButton()}
            </div>
          </div>
        );
    }
    return null;
  };

  const isAvailableDialog = status === 'available';
  const isErrorDialog = status === 'error';
  const canShowDetails = isAvailableDialog || isErrorDialog;

  if (!visible) return null;

  if (presentation === 'compact' || !canShowDetails) {
    return <div className={COMPACT_HOST_CLASS}>{renderCompactContent()}</div>;
  }

  return (
    <NomiModal
      visible
      onCancel={handleClose}
      size={isAvailableDialog ? 'medium' : 'small'}
      style={isAvailableDialog ? { height: '520px' } : { height: 'auto', width: '420px' }}
      header={{
        title: isAvailableDialog ? t('update.availableTitle') : t('update.errorTitle'),
        showClose: true,
      }}
      footer={null}
      contentStyle={{
        height: isAvailableDialog ? '420px' : 'auto',
        padding: 0,
        overflow: isAvailableDialog ? 'hidden' : 'auto',
      }}
    >
      <div className='flex h-full w-full min-h-0 flex-col'>{renderDetailContent()}</div>
    </NomiModal>
  );
};

export default UpdateModal;
