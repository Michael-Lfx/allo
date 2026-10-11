/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Switch } from '@arco-design/web-react';
import { CheckOne, CloseOne } from '@icon-park/react';
import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';
import { prefersReducedMotion } from '@/renderer/utils/motion/flowyMotion';
import {
  SettingsList,
  SettingsPageHeader,
  SettingsRow,
  SettingsSection,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import SettingsPageWrapper from './components/SettingsPageWrapper';

export type VoiceModelInstallStatus =
  | 'NotInstalled'
  | 'ProbingSource'
  | 'Downloading'
  | 'VerifyingSha256'
  | 'Ready'
  | 'Failed'
  | 'Removing';

export interface DownloadingDetail {
  current_file: string;
  file_index: number;
  total_files: number;
  downloaded_bytes: number;
  total_bytes: number;
  progress_percent: number;
  speed_bytes_per_sec: number;
}

export interface VerifyingDetail {
  current_file: string;
  verified_files: number;
  total_files: number;
}

export interface FailedDetail {
  reason: string;
  recoverable: boolean;
}

export interface ReadyDetail {
  models_dir?: string;
  total_size_bytes?: number;
}

export type ModelInstallStateWire =
  | { status: 'NotInstalled' }
  | { status: 'ProbingSource' }
  | { status: 'Downloading'; detail: DownloadingDetail }
  | { status: 'VerifyingSha256'; detail: VerifyingDetail }
  | { status: 'Ready'; detail?: ReadyDetail }
  | { status: 'Failed'; detail: FailedDetail }
  | { status: 'Removing' };

export interface VoiceModelStatusResponse {
  state: ModelInstallStateWire;
  models_dir?: string;
  total_size_bytes?: number;
  files?: Array<{
    path: string;
    size_bytes: number;
    exists: boolean;
  }>;
}

export interface VoiceModelProgressEvent {
  event: string;
  data: ModelInstallStateWire;
}

/** 200ms throttle window per ui-motion.zh.md §3 */
const SSE_PROGRESS_THROTTLE_MS = 200;

const VOICE_ENABLED_STORAGE_KEY = 'nomifun:voice:enabled';

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return '0 B/s';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  const val = (bytesPerSec / Math.pow(k, i)).toFixed(1);
  return `${val} ${sizes[i]}`;
}

const VoiceSettings: React.FC = () => {
  const { t } = useTranslation();
  const [modelStatus, setModelStatus] = useState<VoiceModelInstallStatus>('NotInstalled');
  const [downloadDetail, setDownloadDetail] = useState<DownloadingDetail | null>(null);
  const [verifyDetail, setVerifyDetail] = useState<VerifyingDetail | null>(null);
  const [failedDetail, setFailedDetail] = useState<FailedDetail | null>(null);
  const [voiceEnabled, setVoiceEnabled] = useState<boolean>(() => {
    if (typeof window === 'undefined' || !window.localStorage) return false;
    return window.localStorage.getItem(VOICE_ENABLED_STORAGE_KEY) === 'true';
  });
  const [isStarting, setIsStarting] = useState(false);
  const [loading, setLoading] = useState(true);

  // Motion reduction and ready micro-motion states per ui-motion.zh.md §2 & §3
  const [reducedMotion, setReducedMotion] = useState(() => prefersReducedMotion());
  const [readyIconScaled, setReadyIconScaled] = useState(false);

  // Inline two-step release confirmation state per ui-motion.zh.md §3
  const [isConfirmingRelease, setIsConfirmingRelease] = useState(false);
  const confirmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const lastProgressUpdateRef = useRef<number>(0);
  const abortControllerRef = useRef<AbortController | null>(null);

  // Listen for prefers-reduced-motion OS changes
  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
    const onChange = (e: MediaQueryListEvent) => {
      setReducedMotion(e.matches);
    };
    mediaQuery.addEventListener('change', onChange);
    return () => {
      mediaQuery.removeEventListener('change', onChange);
    };
  }, []);

  // Micro-motion for Ready state: icon scales 0.9 -> 1 over 240ms cubic-bezier(0.16, 1, 0.3, 1)
  useEffect(() => {
    if (modelStatus === 'Ready') {
      setReadyIconScaled(false);
      const frame = requestAnimationFrame(() => {
        setReadyIconScaled(true);
      });
      return () => cancelAnimationFrame(frame);
    } else {
      setReadyIconScaled(false);
    }
  }, [modelStatus]);

  // Clean up release confirmation timer on unmount
  useEffect(() => {
    return () => {
      if (confirmTimerRef.current) {
        clearTimeout(confirmTimerRef.current);
      }
    };
  }, []);

  // Apply SSE state update with 200ms throttle for continuous downloading progress events
  const applyStateUpdate = useCallback((stateWire: ModelInstallStateWire, immediate = false) => {
    const isProgress = stateWire.status === 'Downloading';
    const now = Date.now();
    if (!immediate && isProgress && now - lastProgressUpdateRef.current < SSE_PROGRESS_THROTTLE_MS) {
      return;
    }
    lastProgressUpdateRef.current = now;

    setModelStatus(stateWire.status);
    if (stateWire.status === 'Downloading') {
      setDownloadDetail(stateWire.detail);
    } else if (stateWire.status === 'VerifyingSha256') {
      setVerifyDetail(stateWire.detail);
    } else if (stateWire.status === 'Failed') {
      setFailedDetail(stateWire.detail);
    } else if (stateWire.status === 'Ready') {
      setDownloadDetail(null);
      setVerifyDetail(null);
      setFailedDetail(null);
    } else if (stateWire.status === 'NotInstalled') {
      setDownloadDetail(null);
      setVerifyDetail(null);
      setFailedDetail(null);
    }
  }, []);

  // Subscribe to /api/voice/models/progress SSE stream
  const connectProgressStream = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
    const controller = new AbortController();
    abortControllerRef.current = controller;

    void (async () => {
      try {
        const response = await fetch(`${getBaseUrl()}/api/voice/models/progress`, {
          headers: {
            Accept: 'text/event-stream',
            ...buildBackendAuthHeaders('GET'),
          },
          signal: controller.signal,
        });

        if (!response.ok || !response.body) return;

        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';

          for (const line of lines) {
            const trimmed = line.trim();
            if (trimmed.startsWith('data:')) {
              const jsonStr = trimmed.slice(5).trim();
              if (jsonStr) {
                try {
                  const eventData = JSON.parse(jsonStr) as ModelInstallStateWire | VoiceModelProgressEvent;
                  const statePayload =
                    'data' in eventData && typeof eventData.data === 'object'
                      ? eventData.data
                      : (eventData as ModelInstallStateWire);

                  if (statePayload && typeof statePayload.status === 'string') {
                    applyStateUpdate(statePayload);
                  }
                } catch {
                  // Ignore malformed SSE heartbeat chunks
                }
              }
            }
          }
        }
      } catch {
        // SSE connection closed or aborted, exit gracefully
      }
    })();
  }, [applyStateUpdate]);

  // Initial status fetch on mount
  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models/status`, {
        headers: buildBackendAuthHeaders('GET'),
      });
      if (response.ok) {
        const data = (await response.json()) as VoiceModelStatusResponse;
        if (data?.state?.status) {
          applyStateUpdate(data.state, true);
          // If already in an active transition, keep SSE progress stream subscribed
          if (
            data.state.status === 'ProbingSource' ||
            data.state.status === 'Downloading' ||
            data.state.status === 'VerifyingSha256'
          ) {
            connectProgressStream();
          }
        }
      }
    } catch {
      // Backend status probe is best-effort in skeleton stage; default to NotInstalled
      setModelStatus('NotInstalled');
    } finally {
      setLoading(false);
    }
  }, [applyStateUpdate, connectProgressStream]);

  useEffect(() => {
    void fetchStatus();
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
    };
  }, [fetchStatus]);

  // Trigger POST /api/voice/models/install
  const handleInstall = useCallback(async () => {
    setIsStarting(true);
    try {
      connectProgressStream();
      const response = await fetch(`${getBaseUrl()}/api/voice/models/install`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...buildBackendAuthHeaders('POST'),
        },
        body: JSON.stringify({ prefer_mirror: 'auto' }),
      });
      if (response.ok || response.status === 202) {
        setModelStatus('ProbingSource');
      } else {
        const errJson = (await response.json().catch(() => ({}))) as { error?: string };
        setFailedDetail({
          reason: errJson.error || t('voice.errors.downloadFailed'),
          recoverable: true,
        });
        setModelStatus('Failed');
      }
    } catch (e) {
      setFailedDetail({
        reason: e instanceof Error ? e.message : t('voice.errors.downloadFailed'),
        recoverable: true,
      });
      setModelStatus('Failed');
    } finally {
      setIsStarting(false);
    }
  }, [connectProgressStream, t]);

  // Trigger DELETE /api/voice/models to free disk space
  const handleDelete = useCallback(async () => {
    setModelStatus('Removing');
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models`, {
        method: 'DELETE',
        headers: buildBackendAuthHeaders('DELETE'),
      });
      if (response.ok || response.status === 204) {
        setModelStatus('NotInstalled');
        setDownloadDetail(null);
        setVerifyDetail(null);
        setFailedDetail(null);
      } else {
        const errJson = (await response.json().catch(() => ({}))) as { error?: string };
        setFailedDetail({
          reason: errJson.error || t('voice.errors.unknownError'),
          recoverable: true,
        });
        setModelStatus('Failed');
      }
    } catch (e) {
      setFailedDetail({
        reason: e instanceof Error ? e.message : t('voice.errors.unknownError'),
        recoverable: true,
      });
      setModelStatus('Failed');
    }
  }, [t]);

  // Inline two-step confirmation per ui-motion.zh.md §3:
  // First click shows "确认释放？" with a 3-second auto-revert timer.
  // Second click within 3 seconds confirms deletion and calls DELETE.
  const handleReleaseClick = useCallback(() => {
    if (!isConfirmingRelease) {
      setIsConfirmingRelease(true);
      if (confirmTimerRef.current) {
        clearTimeout(confirmTimerRef.current);
      }
      confirmTimerRef.current = setTimeout(() => {
        setIsConfirmingRelease(false);
        confirmTimerRef.current = null;
      }, 3000);
    } else {
      if (confirmTimerRef.current) {
        clearTimeout(confirmTimerRef.current);
        confirmTimerRef.current = null;
      }
      setIsConfirmingRelease(false);
      void handleDelete();
    }
  }, [isConfirmingRelease, handleDelete]);

  const handleToggleVoiceEnabled = useCallback((checked: boolean) => {
    setVoiceEnabled(checked);
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.setItem(VOICE_ENABLED_STORAGE_KEY, String(checked));
    }
  }, []);

  // Hairline track scaleX computation per ui-motion.zh.md §3
  let progressRatio = 0;
  if (modelStatus === 'Downloading' && downloadDetail) {
    progressRatio = Math.max(0, Math.min(1, (downloadDetail.progress_percent || 0) / 100));
  } else if (modelStatus === 'VerifyingSha256' && verifyDetail) {
    progressRatio = Math.max(0, Math.min(1, (verifyDetail.verified_files || 0) / (verifyDetail.total_files || 7)));
  } else if (modelStatus === 'Ready') {
    progressRatio = 1;
  }

  const isProgressVisible =
    modelStatus === 'ProbingSource' ||
    modelStatus === 'Downloading' ||
    modelStatus === 'VerifyingSha256';

  // Dynamic row label and description per status
  let rowLabel = t('voice.phases.notInstalled');
  let rowDescription: React.ReactNode = t('voice.phases.notInstalledDesc');

  if (modelStatus === 'ProbingSource') {
    rowLabel = t('voice.phases.probingSource');
    rowDescription = t('voice.status.probingSource');
  } else if (modelStatus === 'Downloading') {
    const cur = downloadDetail?.file_index ?? 1;
    const total = downloadDetail?.total_files ?? 7;
    rowLabel = t('voice.phases.downloading', { current: cur, total });
    rowDescription = downloadDetail?.current_file
      ? t('voice.phases.downloadingFile', { file: downloadDetail.current_file, current: cur, total })
      : t('voice.phases.downloading', { current: cur, total });
  } else if (modelStatus === 'VerifyingSha256') {
    const cur = verifyDetail?.verified_files ?? 0;
    const total = verifyDetail?.total_files ?? 7;
    rowLabel = t('voice.phases.verifyingSha256', { current: cur, total });
    rowDescription = t('voice.phases.verifyingSha256', { current: cur, total });
  } else if (modelStatus === 'Ready') {
    rowLabel = t('voice.status.ready');
    rowDescription = t('voice.phases.readyDesc');
  } else if (modelStatus === 'Failed') {
    rowLabel = t('voice.status.failed');
    rowDescription = (
      <span className='text-danger-6'>
        {failedDetail?.reason || t('voice.errors.downloadFailed')}
      </span>
    );
  } else if (modelStatus === 'Removing') {
    rowLabel = t('voice.phases.removing');
    rowDescription = t('voice.actions.releasing');
  }

  return (
    <SettingsPageWrapper layout='form'>
      <div className='flex flex-col gap-24px'>
        <SettingsPageHeader
          title={t('voice.title')}
          description={t('voice.settings.sectionDesc')}
        />

        <SettingsSection
          title={t('voice.settings.sectionTitle')}
          description={t('voice.settings.sectionDesc')}
        >
          <SettingsList>
            {/*
              Geometric Invariance: constant row container height,
              preventing layout shifts across all 7 ModelInstallState phases.
            */}
            <div className='relative overflow-hidden min-h-68px flex items-center'>
              <div className='w-full'>
                <SettingsRow
                  label={
                    <div className='flex items-center gap-8px'>
                      {modelStatus === 'Ready' && (
                        <CheckOne
                          theme='filled'
                          size='16'
                          className='text-primary-6'
                          style={{
                            transition: reducedMotion
                              ? 'none'
                              : 'transform 240ms cubic-bezier(0.16, 1, 0.3, 1)',
                            transform: reducedMotion
                              ? 'scale(1)'
                              : (readyIconScaled ? 'scale(1)' : 'scale(0.9)'),
                          }}
                        />
                      )}
                      {modelStatus === 'Failed' && (
                        <CloseOne
                          theme='filled'
                          size='16'
                          className='text-danger-6'
                        />
                      )}
                      <span>{rowLabel}</span>
                    </div>
                  }
                  description={rowDescription}
                  controlLayout='actions'
                  control={
                    <div className='w-240px shrink-0 flex items-center justify-end font-variant-numeric tabular-nums'>
                      {modelStatus === 'NotInstalled' && (
                        <Button
                          type='primary'
                          loading={isStarting}
                          disabled={loading}
                          onClick={handleInstall}
                          className='tabular-nums'
                        >
                          {t('voice.actions.downloadAndEnable')}
                        </Button>
                      )}

                      {modelStatus === 'ProbingSource' && (
                        <Button type='primary' loading disabled className='tabular-nums'>
                          {t('voice.actions.downloadAndEnable')}
                        </Button>
                      )}

                      {modelStatus === 'Downloading' && (
                        <div className='flex items-center gap-12px text-13px text-t-secondary'>
                          <span className='font-500 text-t-primary'>
                            {t('voice.progress.percent', {
                              percent: Math.round(downloadDetail?.progress_percent ?? 0),
                            })}
                          </span>
                          <span>{formatSpeed(downloadDetail?.speed_bytes_per_sec ?? 0)}</span>
                        </div>
                      )}

                      {modelStatus === 'VerifyingSha256' && (
                        <div className='text-13px text-t-secondary font-500'>
                          {t('voice.progress.ratio', {
                            current: verifyDetail?.verified_files ?? 0,
                            total: verifyDetail?.total_files ?? 7,
                          })}
                        </div>
                      )}

                      {modelStatus === 'Ready' && (
                        <div className='flex items-center gap-12px'>
                          <Switch checked={voiceEnabled} onChange={handleToggleVoiceEnabled} />
                          <Button
                            type={isConfirmingRelease ? 'secondary' : 'text'}
                            status={isConfirmingRelease ? 'danger' : 'default'}
                            size='small'
                            className={isConfirmingRelease ? 'tabular-nums text-danger-6' : 'text-t-secondary'}
                            onClick={handleReleaseClick}
                          >
                            {isConfirmingRelease
                              ? t('voice.actions.confirmRelease')
                              : t('voice.actions.releaseSpace')}
                          </Button>
                        </div>
                      )}

                      {modelStatus === 'Failed' && (
                        <Button
                          type='primary'
                          loading={isStarting}
                          onClick={handleInstall}
                          className='tabular-nums'
                        >
                          {t('voice.actions.retry')}
                        </Button>
                      )}

                      {modelStatus === 'Removing' && (
                        <Button loading disabled size='small'>
                          {t('voice.actions.releaseSpace')}
                        </Button>
                      )}
                    </div>
                  }
                />
              </div>

              {/*
                ui-motion.zh.md §3 contract:
                2px hairline track permanently rendered at the bottom of the row.
                Driven solely by transform: scaleX() (strictly never animates width/height).
              */}
              <div
                aria-hidden='true'
                className='pointer-events-none absolute bottom-0 left-0 right-0 h-2px bg-fill-2'
                style={{
                  opacity: isProgressVisible ? 1 : 0,
                  transition: reducedMotion ? 'none' : 'opacity 180ms ease-out',
                }}
              >
                <div
                  className='h-full bg-primary-6'
                  style={{
                    transform: `scaleX(${progressRatio})`,
                    transformOrigin: 'left center',
                    transition: reducedMotion ? 'none' : 'transform 180ms cubic-bezier(0.16, 1, 0.3, 1)',
                  }}
                />
              </div>
            </div>
          </SettingsList>
        </SettingsSection>
      </div>
    </SettingsPageWrapper>
  );
};

export default VoiceSettings;
