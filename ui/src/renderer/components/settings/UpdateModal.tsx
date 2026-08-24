

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Progress } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { CheckOne, Download, FolderOpen, Refresh, CloseOne, Install } from '@icon-park/react';
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

const BAIDU_RELEASE_MIRROR_URL = 'https://pan.baidu.com/s/5GPonoJNrwJ7GciBSDgXLaA';

const COMPACT_PANEL_CLASS = 'flex flex-col items-center px-24px pt-24px pb-28px';
const COMPACT_ICON_WRAP_CLASS =
  'mb-12px flex h-48px w-48px shrink-0 items-center justify-center rounded-full';

const CompactUpdatePanel: React.FC<{
  icon: React.ReactNode;
  iconWrapClassName?: string;
  title: string;
  description?: React.ReactNode;
  extra?: React.ReactNode;
  actions?: React.ReactNode;
}> = ({ icon, iconWrapClassName, title, description, extra, actions }) => (
  <div className={COMPACT_PANEL_CLASS}>
    <div className={`${COMPACT_ICON_WRAP_CLASS} ${iconWrapClassName ?? ''}`.trim()}>{icon}</div>
    <div className='text-15px font-600 text-t-primary'>{title}</div>
    {description ? (
      <div className='mt-8px max-w-240px text-center text-12px leading-18px text-t-secondary'>{description}</div>
    ) : null}
    {extra}
    {actions ? <div className='mt-20px flex flex-wrap justify-center gap-8px'>{actions}</div> : null}
  </div>
);

const UpdateModal: React.FC = () => {
  const { t } = useTranslation();
  /** Bundled Tauri shell — in-app OTA via ModelScope + tauri-plugin-updater only. */
  const isNativeUpdater = isDesktopShell();
  const [visible, setVisible] = useState(false);
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

  const renderContent = () => {
    switch (status) {
      case 'checking':
        return (
          <CompactUpdatePanel
            icon={
              <div className='relative h-48px w-48px'>
                {/* 环形 spinner：border-3 是颜色类（--bg-3）而不是 3px 宽度，配上本仓库没有
                    border-style 全局重置，两层圆环一条边都画不出来。宽度/样式/颜色分开写。
                    `border-3` is a colour (--bg-3), not a width — with no border-style
                    reset in this repo both rings painted nothing. */}
                <div className='absolute inset-0 rounded-full border-3px border-solid border-[var(--color-fill-3)]' />
                <div className='absolute inset-0 animate-spin rounded-full border-3px border-solid border-primary border-t-transparent' />
              </div>
            }
            title={t('update.checking')}
            actions={renderBaiduManualDownloadButton()}
          />
        );

      case 'upToDate':
        return (
          <CompactUpdatePanel
            icon={<CheckOne theme='filled' size='24' fill='rgb(var(--success-6))' />}
            iconWrapClassName='bg-[rgb(var(--success-6))]/12'
            title={t('update.upToDateTitle')}
            description={t('update.currentVersion', { version: currentVersion || '-' })}
            actions={renderBaiduManualDownloadButton()}
          />
        );

      case 'available':
        return (
          <div className='flex flex-col h-full'>
            {/* Version info header */}
            <div className='flex items-center justify-between px-24px py-16px border-b border-b-solid border-arco-2 bg-fill-1'>
              <div className='flex items-center gap-12px'>
                <div className='w-40px h-40px bg-[rgb(var(--primary-6))]/12 rounded-10px flex items-center justify-center'>
                  <Download size='20' fill='rgb(var(--primary-6))' />
                </div>
                <div>
                  <div className='text-15px font-600 text-t-primary'>{t('update.availableTitle')}</div>
                  <div className='text-12px text-t-tertiary mt-2px'>
                    {currentVersion} →{' '}
                    <span className='text-[rgb(var(--primary-6))] font-500'>
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
              <div className='mx-24px mt-12px px-12px py-10px text-12px rounded-8px bg-[rgb(var(--warning-6))]/10 text-[rgb(var(--warning-6))]'>
                {t('update.noCompatibleAssetManual')}
              </div>
            )}

            {renderManualDownloadHints()}

            {/* Release notes content */}
            <div className='min-h-0 flex-1 overflow-y-auto px-24px py-16px custom-scrollbar'>
              {updateInfo?.name && <div className='text-14px font-500 text-t-primary mb-12px'>{updateInfo.name}</div>}
              {updateInfo?.body || autoUpdateInfo?.releaseNotes ? (
                <div className='text-13px text-t-secondary leading-relaxed'>
                  <MarkdownView allowHtml>{updateInfo?.body || autoUpdateInfo?.releaseNotes || ''}</MarkdownView>
                </div>
              ) : (
                <div className='text-13px text-t-tertiary italic'>{t('update.noReleaseNotes')}</div>
              )}
            </div>
          </div>
        );

      case 'downloading':
        return (
          <CompactUpdatePanel
            icon={<Download size='22' fill='rgb(var(--primary-6))' className='animate-bounce' />}
            iconWrapClassName='bg-[rgb(var(--primary-6))]/12'
            title={t('update.downloadingTitle')}
            extra={
              <div className='mt-16px w-full max-w-280px'>
                <Progress
                  percent={progress.percent}
                  status='normal'
                  showText={false}
                  strokeWidth={6}
                  className='!mb-8px'
                />
                <div className='flex justify-between text-12px text-t-tertiary'>
                  <span>
                    {progress.total > 0
                      ? `${formatSize(progress.transferred)} / ${formatSize(progress.total)}`
                      : formatSize(progress.transferred)}
                  </span>
                  <span className='font-500 text-[rgb(var(--primary-6))]'>{progress.speed}</span>
                </div>
              </div>
            }
            actions={renderBaiduManualDownloadButton()}
          />
        );

      case 'downloaded':
        return (
          <CompactUpdatePanel
            icon={<Install size='22' fill='rgb(var(--primary-6))' />}
            iconWrapClassName='bg-[rgb(var(--primary-6))]/12'
            title={t('update.readyToInstall')}
            description={t('update.installWarning')}
            actions={
              <>
                <Button type='primary' onClick={quitAndInstall} className='flowy-icon-text-btn !px-20px min-w-128px'>
                  <span className='flowy-button-inline-content inline-flex items-center' style={{ gap: 10 }}>
                    <Install theme='outline' size='16' fill='currentColor' strokeWidth={3} />
                    {t('update.installNow')}
                  </span>
                </Button>
                {renderBaiduManualDownloadButton()}
              </>
            }
          />
        );

      case 'installing': {
        const isHandingOff = installPhase === 'installing';
        return (
          <CompactUpdatePanel
            icon={
              <div className='relative h-48px w-48px'>
                <div className='absolute inset-0 rounded-full border-3px border-solid border-[var(--color-fill-3)]' />
                <div className='absolute inset-0 animate-spin rounded-full border-3px border-solid border-primary border-t-transparent' />
                <div className='absolute inset-0 flex items-center justify-center'>
                  <Install size='18' fill='rgb(var(--primary-6))' />
                </div>
              </div>
            }
            title={isHandingOff ? t('update.installingTitle') : t('update.preparingInstallTitle')}
            description={
              <span aria-live='polite'>
                {isHandingOff ? t('update.installingDesc') : t('update.preparingInstallDesc')}
              </span>
            }
          />
        );
      }

      case 'success':
        return (
          <CompactUpdatePanel
            icon={<CheckOne theme='filled' size='24' fill='rgb(var(--success-6))' />}
            iconWrapClassName='bg-[rgb(var(--success-6))]/12'
            title={t('update.downloadCompleteTitle')}
            description={<span className='break-all line-clamp-2'>{downloadPath}</span>}
            actions={
              <>
                <Button size='small' onClick={showInFolder} icon={<FolderOpen size='14' />} className='flowy-icon-text-btn !px-16px'>
                  {t('update.showInFolder')}
                </Button>
                <Button type='primary' size='small' onClick={openFile} className='!px-16px'>
                  {t('update.openFile')}
                </Button>
                {renderBaiduManualDownloadButton()}
              </>
            }
          />
        );

      case 'error':
        return (
          <CompactUpdatePanel
            icon={<CloseOne theme='filled' size='24' fill='rgb(var(--danger-6))' />}
            iconWrapClassName='bg-[rgb(var(--danger-6))]/12'
            title={t('update.errorTitle')}
            description={errorMsg}
            actions={
              <>
                <Button onClick={checkForUpdates} icon={<Refresh size='16' />} className='flowy-icon-text-btn !px-20px'>
                  {t('common.retry')}
                </Button>
                {!isNativeUpdater && renderBaiduManualDownloadButton()}
              </>
            }
          />
        );
    }
  };

  const showReleaseNotes = status === 'available';

  return (
    <NomiModal
      visible={visible}
      onCancel={handleClose}
      size={showReleaseNotes ? 'medium' : 'small'}
      style={showReleaseNotes ? { height: '520px' } : { height: 'auto', width: '360px' }}
      header={{
        title: t('update.modalTitle'),
        showClose: status !== 'installing',
      }}
      footer={null}
      contentStyle={{
        height: showReleaseNotes ? '420px' : 'auto',
        padding: 0,
        overflow: showReleaseNotes ? 'hidden' : 'auto',
      }}
    >
      {showReleaseNotes ? (
        <div className='flex h-full w-full flex-col'>
          <div className='min-h-0 flex-1'>{renderContent()}</div>
        </div>
      ) : (
        renderContent()
      )}
    </NomiModal>
  );
};

export default UpdateModal;
