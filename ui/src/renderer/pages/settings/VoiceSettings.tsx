/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Message, Switch } from '@arco-design/web-react';
import { CheckOne, CloseOne, Info } from '@icon-park/react';
import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';
import { requestDictationIntent } from '@/renderer/pages/guid/dictationIntent';
import { getVoiceCapabilities, parseVoiceModelOperation, parseVoiceModelProgressLine, runVoiceSelfCheck, subscribeVoiceSession, voiceServiceAvailability, wakeVoiceSession, type VoiceCapabilityReport, type VoiceSnapshot } from '@common/adapter/voiceCompanion';
import VoiceAudioStatus from '@renderer/pages/voiceSession/VoiceAudioStatus';
import VoiceTaskDiagnostics from '@renderer/pages/voiceSession/VoiceTaskDiagnostics';
import { voiceErrorKey, voiceErrorTranslationKey } from '@renderer/pages/voiceSession/VoiceErrorStatus';
import VoiceFeedbackSettingsSection from './VoiceFeedbackSettings';
import VoiceDialogueStatusSection from './VoiceDialogueStatus';
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

export interface VoiceModelInstallResponse {
  accepted: boolean;
  state: ModelInstallStateWire;
  message?: string;
}

export interface VoiceModelDeleteResponse {
  success: boolean;
  freed_bytes: number;
  state: ModelInstallStateWire;
  message?: string;
}

/** 200ms throttle window per ui-motion.zh.md §3 */
const SSE_PROGRESS_THROTTLE_MS = 200;


export interface VoiceDeviceStatus {
  mic_available: boolean;
  device_name?: string;
  sample_rate?: number;
  channels?: number;
  error?: string;
}

export interface VoiceAsrTestResult {
  success: boolean;
  text: string;
  duration_ms: number;
  wake_matched?: boolean;
  clean_text?: string;
  error?: string;
}

export function isWakePhraseMatch(text: string): boolean {
  if (!text) return false;
  const clean = text
    .toLowerCase()
    .replace(/[，。！？；：“”‘’（）【】、—….,!?;:"'()[\]\-_]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

  const patterns = [
    'hey flowy',
    'hey follow',
    'hey flow',
    'hey f',
    'hi flowy',
    'hi follow',
    'hi flow',
    'hello flowy',
    'hello flow',
    'flowy',
    'floy',
    '黑 flowy',
    '嘿 flowy',
    '嘿flowy',
    '嘿 flow',
    '嘿flow',
    '你好 flowy',
    '你好flowy',
    '小 flowy',
    '小flowy',
    'hey 芙',
    'hey芙',
    '嘿 芙',
    '嘿芙',
    'hey 弗',
    'hey弗',
    'hey 福',
    'hey福',
    'hey 浮',
    'hey浮',
    '嘿 浮',
    '嘿浮',
    'hey 伏',
    'hey伏',
    'hey 符',
    'hey符',
    'hey 服',
    'hey服',
    'hey 佛',
    'hey佛',
    'hey 飞',
    'hey飞',
    '嗨 flowy',
    '嗨flowy',
    '嗨 flow',
    '嗨flow',
    '嗨 芙',
    '嗨芙',
    '黑 芙',
    '黑芙',
    'hey 楼',
    'hey楼',
    'hey 洛',
    'hey洛',
  ];

  return patterns.some((p) => clean.includes(p));
}

export interface DashboardLogEntry {
  id: string;
  time: string;
  text: string;
  type: 'info' | 'voice' | 'wake';
}

export type KwsLiveStatus =
  | 'idle'
  | 'listening'
  | 'speech'
  | 'wake_hit'
  | 'deciding'
  | 'speaking'
  | 'follow_up';

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return '0 B/s';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  const val = (bytesPerSec / Math.pow(k, i)).toFixed(1);
  return `${val} ${sizes[i]}`;
}

function writeString(view: DataView, offset: number, string: string): void {
  for (let i = 0; i < string.length; i++) {
    view.setUint8(offset + i, string.charCodeAt(i));
  }
}

function encodeWavPcm(samples: Float32Array, sampleRate = 16000): ArrayBuffer {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);

  // RIFF chunk descriptor
  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  writeString(view, 8, 'WAVE');

  // fmt sub-chunk
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM = 1
  view.setUint16(22, 1, true); // Mono = 1
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);

  // data sub-chunk
  writeString(view, 36, 'data');
  view.setUint32(40, samples.length * 2, true);

  let offset = 44;
  for (let i = 0; i < samples.length; i++, offset += 2) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }

  return buffer;
}

export const VoiceModelOperationAlert: React.FC<{ message: string | null; hidden?: boolean }> = ({
  message,
  hidden = false,
}) =>
  message && !hidden ? (
    <div
      role='alert'
      className='border-t border-t-solid border-[var(--border-base)] bg-fill-1 px-4 py-3 text-12px text-danger-6'
    >
      {message}
    </div>
  ) : null;
const VoiceSettings: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [modelStatus, setModelStatus] = useState<VoiceModelInstallStatus>('NotInstalled');
  const [downloadDetail, setDownloadDetail] = useState<DownloadingDetail | null>(null);
  const [verifyDetail, setVerifyDetail] = useState<VerifyingDetail | null>(null);
  const [failedDetail, setFailedDetail] = useState<FailedDetail | null>(null);
  const [voiceEnabled, setVoiceEnabled] = useState(false);
  const [voiceSnapshot, setVoiceSnapshot] = useState<VoiceSnapshot | null>(null);
  const [voiceSyncError, setVoiceSyncError] = useState<string | null>(null);
  const [voiceStatusRefresh, setVoiceStatusRefresh] = useState(0);
  const [waking, setWaking] = useState(false);
  const [isTogglingCompanion, setIsTogglingCompanion] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [loading, setLoading] = useState(true);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [capabilities, setCapabilities] = useState<VoiceCapabilityReport | null>(null);
  const [checkingCapabilities, setCheckingCapabilities] = useState(false);

  // Diagnostics & testing state
  const [micStatus, setMicStatus] = useState<VoiceDeviceStatus | null>(null);
  const [micChecking, setMicChecking] = useState(false);
  const [asrRunning, setAsrRunning] = useState(false);
  const [asrRecording, setAsrRecording] = useState(false);
  const [asrResult, setAsrResult] = useState<VoiceAsrTestResult | null>(null);
  const asrTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeStreamRef = useRef<MediaStream | null>(null);

  // Live Dashboard & Acoustic Monitor state
  const [kwsLiveStatus, setKwsLiveStatus] = useState<KwsLiveStatus>('idle');
  const [dashboardLogs, setDashboardLogs] = useState<DashboardLogEntry[]>([]);

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

  // Clean up timers and audio streams on unmount
  useEffect(() => {
    return () => {
      if (confirmTimerRef.current) {
        clearTimeout(confirmTimerRef.current);
      }
      if (asrTimerRef.current) {
        clearTimeout(asrTimerRef.current);
      }
      if (activeStreamRef.current) {
        activeStreamRef.current.getTracks().forEach((track) => track.stop());
        activeStreamRef.current = null;
      }
    };
  }, []);

  // Observe the host-owned session; opening settings never acquires the microphone.
  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    let previousSnapshot: VoiceSnapshot | null = null;
    void subscribeVoiceSession((snapshot) => {
      if (disposed) return;
      setVoiceEnabled(snapshot.enabled);
      setVoiceSnapshot(snapshot);
      const phase = snapshot.phase;
      setKwsLiveStatus(phase === 'speaking' ? 'speaking' : phase === 'follow_up' || phase === 'text_follow_up' ? 'follow_up' : phase === 'deciding' || phase === 'transcribing' || phase === 'synthesizing' ? 'deciding' : phase === 'listening' ? 'listening' : 'idle');
      const entries: string[] = [];
      if (previousSnapshot?.phase !== phase) entries.push(t(`voice.session.phase.${phase}`));
      if (snapshot.user_text && (previousSnapshot?.turn_id !== snapshot.turn_id || previousSnapshot.user_text !== snapshot.user_text)) entries.push(t('voice.dashboard.logUserSaid', { text: snapshot.user_text }));
      if (snapshot.understood_request && previousSnapshot?.understood_request !== snapshot.understood_request) entries.push(`${t('voice.session.understoodRequest')}: ${snapshot.understood_request}`);
      if (snapshot.assistant_text && previousSnapshot?.assistant_text !== snapshot.assistant_text) entries.push(`${t('voice.session.assistantText')}: ${snapshot.assistant_text}`);
      if (snapshot.error && previousSnapshot?.error?.code !== snapshot.error.code) entries.push(t(voiceErrorKey(snapshot.error.code)));
      previousSnapshot = snapshot;
      if (entries.length) setDashboardLogs((previous) => [...entries.reverse().map((text, index) => ({ id: `${snapshot.instance_id}-${snapshot.seq}-${index}`, time: new Date().toTimeString().slice(0, 8), text, type: 'info' as const })), ...previous].slice(0, 30));
    }, false, (code) => { if (!disposed) setVoiceSyncError(code); }).then((off) => { if (disposed) off(); else unsubscribe = off; }).catch(() => {
      if (!disposed) { setKwsLiveStatus('idle'); setVoiceSyncError('voice_snapshot_unavailable'); }
    });
    return () => { disposed = true; unsubscribe?.(); };
  }, [t, voiceStatusRefresh]);
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
              const statePayload = parseVoiceModelProgressLine(trimmed);
              if (statePayload) {
                applyStateUpdate(statePayload as ModelInstallStateWire);
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

  const fetchCompanionStatus = useCallback(async () => {
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/companion/status`, {
        headers: buildBackendAuthHeaders('GET'),
      });
      if (response.ok) {
        const data = (await response.json()) as { running?: boolean; autostart?: boolean };
        setVoiceEnabled(Boolean(data.autostart));
      }
    } catch {
      // Best-effort
    }
  }, []);

  useEffect(() => {
    void fetchStatus();
    void fetchCompanionStatus();
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
    };
  }, [fetchStatus, fetchCompanionStatus]);

  const operationErrorText = useCallback((fallback: string, error?: string, code?: string) => {
    if (code === 'voice_suite_in_use') return t('voice.errors.suiteInUse');
    if (code === 'voice_resource_write_in_progress') return t('voice.errors.resourceBusy');
    return error || fallback;
  }, [t]);

  // Trigger POST /api/voice/models/install
  const handleInstall = useCallback(async (force = false) => {
    setIsStarting(true);
    setOperationError(null);
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models/install`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...buildBackendAuthHeaders('POST'),
        },
        body: JSON.stringify({ prefer_mirror: 'auto', force: Boolean(force) }),
      });
      // Decode exactly once: a second response.json() would consume the body and
      // erase the backend's 409 / removal-failure reason.
      const outcome = parseVoiceModelOperation('install', response.status, await response.text());
      if (!outcome.ok) {
        setOperationError(operationErrorText(t('voice.errors.downloadFailed'), outcome.error, outcome.code));
        return;
      }
      const state = outcome.receipt?.state as ModelInstallStateWire | undefined;
      if (!state) {
        setOperationError(t('voice.errors.operationFailed'));
        return;
      }
      // The backend may reject a duplicate/already-ready request; always render
      // the authoritative state it returned instead of forcing ProbingSource.
      applyStateUpdate(state, true);
      if (outcome.connectProgressStream) {
        connectProgressStream();
      } else if (state.status === 'Failed') {
        setFailedDetail({
          reason: outcome.receipt?.message || t('voice.errors.downloadFailed'),
          recoverable: true,
        });
      }
    } catch (e) {
      setOperationError(e instanceof Error ? e.message : t('voice.errors.downloadFailed'));
    } finally {
      setIsStarting(false);
    }
  }, [applyStateUpdate, connectProgressStream, operationErrorText, t]);

  // Trigger DELETE /api/voice/models to free disk space
  const handleDelete = useCallback(async () => {
    setOperationError(null);
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/models`, {
        method: 'DELETE',
        headers: buildBackendAuthHeaders('DELETE'),
      });
      const outcome = parseVoiceModelOperation('delete', response.status, await response.text());
      if (!outcome.ok) {
        setOperationError(operationErrorText(t('voice.errors.unknownError'), outcome.error, outcome.code));
        return;
      }
      const state = outcome.receipt?.state as ModelInstallStateWire | undefined;
      if (!state) {
        setOperationError(t('voice.errors.operationFailed'));
        return;
      }
      // Never claim NotInstalled from a bare 2xx: honor the deletion receipt.
      applyStateUpdate(state, true);
      if (outcome.receipt?.success) {
        setDownloadDetail(null);
        setVerifyDetail(null);
        setFailedDetail(null);
      } else {
        setOperationError(t('voice.errors.deleteIncomplete', {
          reason: outcome.receipt?.message || t('voice.errors.unknownError'),
        }));
      }
    } catch (e) {
      setOperationError(e instanceof Error ? e.message : t('voice.errors.unknownError'));
    }
  }, [applyStateUpdate, operationErrorText, t]);

  // Inline two-step confirmation per ui-motion.zh.md §3:
  // First click shows "确认释放？" with a 3-second auto-revert timer.
  // Second click within 3 seconds confirms deletion and calls DELETE.
  const fetchCapabilities = useCallback(async () => {
    try {
      setCapabilities(await getVoiceCapabilities());
    } catch {
      setCapabilities(null);
    }
  }, []);

  const handleSelfCheck = useCallback(async () => {
    setCheckingCapabilities(true);
    try {
      // Explicit user action: may make one short, tool-free request through the
      // configured dialogue provider and synthesize fixed check sentences.
      const report = await runVoiceSelfCheck(true);
      setCapabilities(report);
      if (report.can_start_full_call) {
        Message.success(t('voice.capabilities.fullReady'));
      } else {
        Message.warning(t('voice.capabilities.notReady'));
      }
    } catch (e) {
      Message.error(e instanceof Error ? e.message : t('voice.capabilities.checkFailed'));
    } finally {
      setCheckingCapabilities(false);
    }
  }, [t]);

  const handleUseTextReply = useCallback(() => {
    const intent = requestDictationIntent('/');
    navigate('/', { state: { dictationIntentId: intent.id } });
  }, [navigate]);
  useEffect(() => {
    if (modelStatus === 'Ready') {
      void fetchCapabilities();
    }
  }, [modelStatus, fetchCapabilities]);

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

  const handleToggleVoiceEnabled = useCallback(async (checked: boolean) => {
    setIsTogglingCompanion(true);
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/companion/toggle`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...buildBackendAuthHeaders('POST'),
        },
        body: JSON.stringify({ enabled: checked }),
      });
      if (response.ok) {
        const data = (await response.json()) as { success: boolean; running: boolean; error?: string };
        if (data.success) {
          setVoiceEnabled(checked);
        } else if (data.error) {
          const translationKey = voiceErrorTranslationKey(data.error);
          Message.error(translationKey ? t(translationKey) : data.error);
        }
      } else {
        const errJson = (await response.json().catch(() => ({}))) as { error?: string };
        Message.error(errJson.error || t('voice.errors.unknownError'));
      }
    } catch (e) {
      Message.error(e instanceof Error ? e.message : t('voice.errors.unknownError'));
    } finally {
      setIsTogglingCompanion(false);
      void fetchCompanionStatus();
    }
  }, [t, fetchCompanionStatus]);

  const fetchDeviceStatus = useCallback(async () => {
    setMicChecking(true);
    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/diagnostics/device`, {
        headers: buildBackendAuthHeaders('GET'),
      });
      if (response.ok) {
        const data = (await response.json()) as VoiceDeviceStatus;
        setMicStatus(data);
      }
    } catch {
      setMicStatus({ mic_available: false, error: 'Connection failed' });
    } finally {
      setMicChecking(false);
    }
  }, []);

  const handleStartAsrTest = useCallback(async () => {
    setAsrRunning(true);
    setAsrResult(null);

    let audioBase64: string | undefined = undefined;
    let recordedSampleRate = 16000;
    if (typeof window !== 'undefined' && navigator.mediaDevices?.getUserMedia) {
      try {
        setAsrRecording(true);
        const AudioContextClass =
          window.AudioContext ||
          (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;

        if (AudioContextClass) {
          const audioCtx = new AudioContextClass({ sampleRate: 16000 });
          const actualSampleRate = audioCtx.sampleRate || 16000;
          const stream = await navigator.mediaDevices.getUserMedia({
            audio: {
              channelCount: 1,
              sampleRate: actualSampleRate,
              echoCancellation: true,
              noiseSuppression: true,
            },
          });
          activeStreamRef.current = stream;

          const source = audioCtx.createMediaStreamSource(stream);
          const processor = audioCtx.createScriptProcessor(4096, 1, 1);
          const pcmChunks: Float32Array[] = [];

          processor.onaudioprocess = (e) => {
            const input = e.inputBuffer.getChannelData(0);
            pcmChunks.push(new Float32Array(input));
          };

          source.connect(processor);
          processor.connect(audioCtx.destination);

          // Record for 3000ms
          await new Promise<void>((resolve) => {
            asrTimerRef.current = setTimeout(() => {
              asrTimerRef.current = null;
              resolve();
            }, 3000);
          });

          // Disconnect and release tracks
          processor.disconnect();
          source.disconnect();
          stream.getTracks().forEach((track) => track.stop());
          activeStreamRef.current = null;
          await audioCtx.close();

          const totalLength = pcmChunks.reduce((acc, c) => acc + c.length, 0);
          const merged = new Float32Array(totalLength);
          let offset = 0;
          for (const chunk of pcmChunks) {
            merged.set(chunk, offset);
            offset += chunk.length;
          }

          if (merged.length > 0) {
            const wavBuffer = encodeWavPcm(merged, actualSampleRate);
            const blob = new Blob([wavBuffer], { type: 'audio/wav' });
            const reader = new FileReader();
            audioBase64 = await new Promise<string>((resolve) => {
              reader.onloadend = () => {
                resolve((reader.result as string) || '');
              };
              reader.readAsDataURL(blob);
            });
            recordedSampleRate = actualSampleRate;
          }
        }
      } catch {
        // Fallback to backend physical microphone if browser permission denied
      } finally {
        setAsrRecording(false);
      }
    }

    try {
      const response = await fetch(`${getBaseUrl()}/api/voice/diagnostics/asr-test`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...buildBackendAuthHeaders('POST'),
        },
        body: JSON.stringify({
          audio_base64: audioBase64,
          sample_rate: recordedSampleRate,
        }),
      });
      if (response.ok) {
        const data = (await response.json()) as VoiceAsrTestResult;
        setAsrResult(data);
      } else {
        const errJson = (await response.json().catch(() => ({}))) as { error?: string };
        setAsrResult({
          success: false,
          text: '',
          duration_ms: 0,
          error: errJson.error || 'Request failed',
        });
      }
    } catch (e) {
      setAsrResult({
        success: false,
        text: '',
        duration_ms: 0,
        error: e instanceof Error ? e.message : 'Network request failed',
      });
    } finally {
      setAsrRunning(false);
      setAsrRecording(false);
    }
  }, []);

  useEffect(() => {
    if (modelStatus === 'Ready') {
      void fetchDeviceStatus();
    }
  }, [modelStatus, fetchDeviceStatus]);

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
  const serviceAvailability = voiceServiceAvailability(voiceSnapshot, voiceEnabled, voiceSyncError);
  const serviceDescription = serviceAvailability === 'syncFailed' && voiceSyncError === 'voice_status_forbidden'
    ? t('voice.service.authSyncFailedDesc')
    : serviceAvailability === 'syncFailed' && voiceSyncError === 'voice_status_api_missing'
      ? t('voice.service.apiMissingDesc')
      : t(`voice.service.${serviceAvailability}Desc`);
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
    rowLabel = t(`voice.service.${serviceAvailability}`);
    rowDescription = serviceDescription;
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
                          onClick={() => handleInstall(false)}
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
                          <Button
                            type='primary'
                            size='small'
                            loading={checkingCapabilities}
                            onClick={handleSelfCheck}
                          >
                            {t('voice.capabilities.run')}
                          </Button>
                          <Switch
                            aria-label={t('voice.service.enableIntent')}
                            checked={voiceEnabled}
                            disabled={!voiceEnabled && (!voiceSnapshot || Boolean(voiceSyncError))}
                            loading={isTogglingCompanion}
                            onChange={handleToggleVoiceEnabled}
                          />
                          <Button
                            type='text'
                            size='small'
                            className='text-t-secondary'
                            onClick={() => handleInstall(true)}
                          >
                            {t('voice.actions.repair')}
                          </Button>
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
                          onClick={() => handleInstall(true)}
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

              <VoiceModelOperationAlert message={operationError} hidden={modelStatus === 'Failed'} />

              {/*
                ui-motion.zh.md ��3 contract:
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

            {modelStatus === 'Ready' && capabilities && (
              <div className='flex flex-col gap-2 px-4 pb-3 pt-3 text-12px border-t border-t-solid border-[var(--border-base)]' role='status'>
                {(['capture', 'kws', 'vad', 'asr', 'llm', 'tts', 'playback'] as const).map((name) => {
                  const item = capabilities[name];
                  return (
                    <div key={name} className='flex items-start justify-between gap-3'>
                      <span className='text-t-secondary'>{t(`voice.capabilities.item.${name}`)}</span>
                      <span
                        className={
                          item.status === 'ready'
                            ? 'text-success-6 font-medium'
                            : item.status === 'failed'
                              ? 'text-danger-6'
                              : 'text-t-tertiary'
                        }
                      >
                        {t(`voice.capabilities.status.${item.status}`)}
                        {item.reason_code && item.status !== 'ready'
                          ? ` · ${t(`voice.capabilities.reason.${item.reason_code}`, { defaultValue: item.reason_code })}`
                          : ''}
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </SettingsList>
        </SettingsSection>

        <VoiceFeedbackSettingsSection connected={voiceSnapshot?.connection === 'connected'} />

        <p className='m-0 text-12px text-t-secondary' role='status'>
          {voiceSnapshot?.tts ? t('voice.presentation.ttsActual', {
            provider: voiceSnapshot.tts.provider,
            voice: voiceSnapshot.tts.voice ?? '—',
            location: t(voiceSnapshot.tts.online ? 'voice.presentation.online' : 'voice.presentation.local'),
          }) : t('voice.presentation.ttsUnknown')}
          {voiceSnapshot?.tts?.fallback_reason && ` · ${t('voice.presentation.ttsFallback')}`}
        </p>

        {modelStatus === 'Ready' && (
          <SettingsSection
            title={t('voice.diagnostics.advancedTitle')}
            description={t('voice.diagnostics.advancedDesc')}
          >
            <details className='group rounded-8px border border-solid border-[var(--border-base)] bg-fill-1 p-4 transition-colors'>
              <summary className='cursor-pointer select-none text-13px font-medium text-t-primary flex items-center justify-between'>
                <span className='flex items-center gap-2'>
                  <Info theme='outline' size='16' className='text-t-secondary' />
                  <span>{t('voice.diagnostics.sectionTitle')}</span>
                </span>
                <span className='text-12px text-t-tertiary transition-transform group-open:rotate-180'>▼</span>
              </summary>

              <div className='mt-4 flex flex-col gap-4 border-t border-t-solid border-[var(--border-base)] pt-4'>
                <SettingsList>
                  <SettingsRow
                    label={t('voice.session.tryWake')}
                    description={serviceAvailability === 'ready' ? t('voice.session.tryWakeDescription') : serviceDescription}
                    controlLayout='actions'
                    control={<Button size='small' loading={waking} disabled={serviceAvailability !== 'ready' || asrRunning} onClick={() => {
                      setWaking(true);
                      void wakeVoiceSession().catch(() => Message.error(t('voice.session.unavailable'))).finally(() => setWaking(false));
                    }}>{t('voice.session.tryWake')}</Button>}
                  />
                  <div className='min-h-68px flex items-center'>
                    <div className='w-full'>
                      <SettingsRow
                        label={t('voice.diagnostics.micDevice')}
                        description={
                          micChecking
                            ? t('voice.diagnostics.micDeviceChecking')
                            : micStatus?.mic_available
                              ? t('voice.diagnostics.micDetected', {
                                  name: micStatus.device_name || 'Default Input',
                                  rate: micStatus.sample_rate || 16000,
                                  channels: micStatus.channels || 1,
                                })
                              : micStatus?.error || t('voice.diagnostics.micNotFound')
                        }
                        controlLayout='actions'
                        control={
                          <div className='w-240px shrink-0 flex items-center justify-end font-variant-numeric tabular-nums'>
                            <Button
                              size='small'
                              loading={micChecking}
                              onClick={fetchDeviceStatus}
                            >
                              {t('voice.diagnostics.micDeviceProbe')}
                            </Button>
                          </div>
                        }
                      />
                    </div>
                  </div>

                  <div className='min-h-68px flex items-center'>
                    <div className='w-full'>
                      <SettingsRow
                        label={t('voice.diagnostics.asrTest')}
                        description={
                          asrRunning
                            ? (asrRecording
                                ? t('voice.diagnostics.asrTestRecording')
                                : t('voice.diagnostics.asrTestTranscribing'))
                            : asrResult
                              ? (asrResult.success
                                  ? t('voice.diagnostics.asrTestSuccess', {
                                      text: asrResult.text,
                                      duration: asrResult.duration_ms,
                                    })
                                  : t('voice.diagnostics.asrTestFailed', {
                                      reason: asrResult.error || t('voice.errors.unknownError'),
                                    }))
                              : t('voice.diagnostics.asrTestIdleDesc')
                        }
                        controlLayout='actions'
                        control={
                          <div className='w-240px shrink-0 flex items-center justify-end font-variant-numeric tabular-nums'>
                            <Button
                              type='primary'
                              size='small'
                              loading={asrRunning}
                              onClick={handleStartAsrTest}
                            >
                              {t('voice.diagnostics.asrTestBtn')}
                            </Button>
                          </div>
                        }
                      />
                    </div>
                  </div>
                </SettingsList>

                {voiceEnabled && (
                  <div className='mt-2 flex flex-col gap-4 border-t border-t-solid border-[var(--border-base)] pt-4'>
                    <div className='flex flex-wrap items-center justify-between gap-4'>
                      <div className='flex items-center gap-3'>
                        <div className='relative flex items-center justify-center'>
                          <div
                            className={`h-3 w-3 rounded-full ${
                              kwsLiveStatus === 'wake_hit'
                                ? 'bg-success-6 animate-ping'
                                : kwsLiveStatus === 'speech' || kwsLiveStatus === 'deciding'
                                  ? 'bg-warning-6 animate-pulse'
                                  : kwsLiveStatus === 'speaking' || kwsLiveStatus === 'follow_up'
                                    ? 'bg-primary-6 animate-pulse'
                                    : 'bg-primary-6'
                            }`}
                          />
                          <div
                            className={`absolute h-2 w-2 rounded-full ${
                              kwsLiveStatus === 'wake_hit'
                                ? 'bg-success-6'
                                : kwsLiveStatus === 'speech' || kwsLiveStatus === 'deciding'
                                  ? 'bg-warning-6'
                                  : kwsLiveStatus === 'speaking' || kwsLiveStatus === 'follow_up'
                                    ? 'bg-primary-6'
                                    : 'bg-primary-6'
                            }`}
                          />
                        </div>
                        <div>
                          <div className='text-14px font-medium text-t-primary'>
                            {serviceAvailability === 'ready' && voiceSnapshot ? t(`voice.session.phase.${voiceSnapshot.phase}`) : t(`voice.service.${serviceAvailability}`)}
                          </div>
                          <div className='text-12px text-t-secondary'>
                            {serviceDescription}
                          </div>
                        </div>
                      </div>

                      {(voiceSyncError || !voiceSnapshot) && <Button size='small' onClick={() => setVoiceStatusRefresh((value) => value + 1)}>{t('voice.service.retrySync')}</Button>}
                    </div>

                    <VoiceAudioStatus snapshot={voiceSnapshot} />
                    <VoiceTaskDiagnostics key={`${voiceSnapshot?.instance_id}:${voiceSnapshot?.binding?.conversation_id}:${voiceSnapshot?.task?.command_id}`} snapshot={voiceSnapshot} />

                    <div className='flex flex-col gap-2'>
                      <div className='flex items-center justify-between text-12px text-t-secondary'>
                        <span className='font-medium text-t-primary'>{t('voice.dashboard.logTitle')}</span>
                        <button
                          type='button'
                          className='text-11px text-t-tertiary hover:text-primary-6 transition-colors bg-transparent border-none cursor-pointer p-0'
                          onClick={() => setDashboardLogs([])}
                        >
                          {t('voice.dashboard.clearLog')}
                        </button>
                      </div>
                      <div className='max-h-160px min-h-80px flex flex-col gap-1.5 overflow-y-auto rounded-6px bg-fill-2 p-3 font-mono text-11px text-t-secondary'>
                        {dashboardLogs.length === 0 ? (
                          <div className='text-t-tertiary text-center py-2'>-</div>
                        ) : (
                          dashboardLogs.map((log) => (
                            <div key={log.id} className='flex items-start gap-2 leading-tight'>
                              <span className='text-t-tertiary shrink-0 select-none'>[{log.time}]</span>
                              <span
                                className={
                                  log.type === 'wake'
                                    ? 'text-success-6 font-semibold'
                                    : log.type === 'voice'
                                      ? 'text-warning-6'
                                      : 'text-t-secondary'
                                }
                              >
                                {log.text}
                              </span>
                            </div>
                          ))
                        )}
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </details>
          </SettingsSection>
        )}
      </div>
    </SettingsPageWrapper>
  );
};

export default VoiceSettings;
