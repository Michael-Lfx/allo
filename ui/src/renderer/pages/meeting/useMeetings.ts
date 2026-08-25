import { useCallback, useEffect, useState } from 'react';
import { ipcBridge } from '@/common';
import type {
  MeetingDevice,
  MeetingEvent,
  MeetingSegment,
  MeetingSession,
  MeetingVoiceprint,
  SttBackendChoice,
} from '@/common/adapter/ipcBridge';

const upsertSession = (sessions: MeetingSession[], session: MeetingSession): MeetingSession[] =>
  sessions.some((item) => item.session_id === session.session_id)
    ? sessions.map((item) => (item.session_id === session.session_id ? session : item))
    : [session, ...sessions];

const upsertSegment = (segments: MeetingSegment[], segment: MeetingSegment): MeetingSegment[] => {
  const next = segments.some((item) => item.segment_id === segment.segment_id)
    ? segments.map((item) => (item.segment_id === segment.segment_id ? segment : item))
    : [...segments, segment];
  return next.sort((a, b) => a.start_ms - b.start_ms || a.end_ms - b.end_ms);
};

export type CapabilityDegrade = {
  session_id: string;
  mic_available: boolean;
  loopback_available: boolean;
  message: string;
} | null;

export function useMeetings() {
  const [sessions, setSessions] = useState<MeetingSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [segments, setSegments] = useState<MeetingSegment[]>([]);
  const [devices, setDevices] = useState<MeetingDevice[]>([]);
  const [detectedApp, setDetectedApp] = useState<string | null>(null);
  const [voiceprints, setVoiceprints] = useState<MeetingVoiceprint[]>([]);
  const [capabilityDegrade, setCapabilityDegrade] = useState<CapabilityDegrade>(null);
  const [loading, setLoading] = useState(true);
  const [segmentsLoading, setSegmentsLoading] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const selected = sessions.find((s) => s.session_id === selectedId) ?? null;

  const refreshSessions = useCallback(async () => {
    const list = await ipcBridge.meeting.listSessions.invoke();
    setSessions(list ?? []);
    return list ?? [];
  }, []);

  const refreshDevices = useCallback(async () => {
    try {
      const list = await ipcBridge.meeting.listDevices.invoke();
      setDevices(list ?? []);
    } catch {
      setDevices([]);
    }
  }, []);

  const refreshDetectedApps = useCallback(async () => {
    try {
      const result = await ipcBridge.meeting.detectedApps.invoke();
      setDetectedApp(result?.app ?? null);
    } catch {
      setDetectedApp(null);
    }
  }, []);

  const refreshVoiceprints = useCallback(async () => {
    try {
      const list = await ipcBridge.meeting.listVoiceprints.invoke();
      setVoiceprints(list ?? []);
    } catch {
      setVoiceprints([]);
    }
  }, []);

  const loadSegments = useCallback(async (sessionId: string) => {
    setSegmentsLoading(true);
    try {
      const list = await ipcBridge.meeting.listSegments.invoke({ session_id: sessionId });
      setSegments((list ?? []).slice().sort((a, b) => a.start_ms - b.start_ms || a.end_ms - b.end_ms));
    } catch {
      setSegments([]);
    } finally {
      setSegmentsLoading(false);
    }
  }, []);

  const selectSession = useCallback(
    (sessionId: string | null) => {
      setSelectedId(sessionId);
      setCapabilityDegrade((prev) => (prev && prev.session_id === sessionId ? prev : null));
      if (sessionId) {
        void loadSegments(sessionId);
      } else {
        setSegments([]);
      }
    },
    [loadSegments]
  );

  const bootstrap = useCallback(async () => {
    setLoading(true);
    try {
      const list = await refreshSessions();
      await Promise.all([refreshDevices(), refreshDetectedApps(), refreshVoiceprints()]);
      if (list.length > 0) {
        const first = list[0];
        setSelectedId(first.session_id);
        await loadSegments(first.session_id);
      }
    } finally {
      setLoading(false);
    }
  }, [loadSegments, refreshDetectedApps, refreshDevices, refreshSessions, refreshVoiceprints]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  useEffect(() => {
    const offEvent = ipcBridge.meeting.onEvent.on((event: MeetingEvent) => {
      switch (event.type) {
        case 'session_updated':
          setSessions((prev) => upsertSession(prev, event.session));
          break;
        case 'segment_upserted':
          if (event.segment.session_id === selectedId) {
            setSegments((prev) => upsertSegment(prev, event.segment));
          }
          break;
        case 'capability_degraded':
          setCapabilityDegrade({
            session_id: event.session_id,
            mic_available: event.mic_available,
            loopback_available: event.loopback_available,
            message: event.message,
          });
          setSessions((prev) =>
            prev.map((session) =>
              session.session_id === event.session_id
                ? {
                    ...session,
                    mic_available: event.mic_available,
                    loopback_available: event.loopback_available,
                  }
                : session
            )
          );
          break;
        case 'error':
          setLastError(event.message);
          break;
        default: {
          const _exhaustive: never = event;
          void _exhaustive;
          break;
        }
      }
    });
    const offReconnected = ipcBridge.conversation.reconnected.on(() => {
      void refreshSessions().then((list) => {
        if (selectedId && list.some((s) => s.session_id === selectedId)) {
          void loadSegments(selectedId);
        }
      });
      void refreshDevices();
      void refreshDetectedApps();
    });
    return () => {
      offEvent();
      offReconnected();
    };
  }, [
    loadSegments,
    refreshDetectedApps,
    refreshDevices,
    refreshSessions,
    selectedId,
  ]);

  const createSession = useCallback(
    async (params: {
      title: string;
      stt_backend: SttBackendChoice;
      bound_conversation_id?: string | null;
    }) => {
      const session = await ipcBridge.meeting.createSession.invoke({
        title: params.title.trim() || undefined,
        stt_backend: params.stt_backend,
        bound_conversation_id: params.bound_conversation_id || null,
      });
      setSessions((prev) => upsertSession(prev, session));
      selectSession(session.session_id);
      return session;
    },
    [selectSession]
  );

  const startSession = useCallback(async (sessionId: string) => {
    const session = await ipcBridge.meeting.start.invoke({ session_id: sessionId });
    setSessions((prev) => upsertSession(prev, session));
    return session;
  }, []);

  const pauseSession = useCallback(async (sessionId: string) => {
    const session = await ipcBridge.meeting.pause.invoke({ session_id: sessionId });
    setSessions((prev) => upsertSession(prev, session));
    return session;
  }, []);

  const resumeSession = useCallback(async (sessionId: string) => {
    const session = await ipcBridge.meeting.resume.invoke({ session_id: sessionId });
    setSessions((prev) => upsertSession(prev, session));
    return session;
  }, []);

  const stopSession = useCallback(async (sessionId: string) => {
    const session = await ipcBridge.meeting.stop.invoke({ session_id: sessionId });
    setSessions((prev) => upsertSession(prev, session));
    return session;
  }, []);

  const bindConversation = useCallback(async (sessionId: string, conversationId: string | null) => {
    const session = await ipcBridge.meeting.bind.invoke({
      session_id: sessionId,
      conversation_id: conversationId,
    });
    setSessions((prev) => upsertSession(prev, session));
    return session;
  }, []);

  const enrollVoiceprint = useCallback(async (displayName: string) => {
    const row = await ipcBridge.meeting.enrollVoiceprint.invoke({ display_name: displayName });
    setVoiceprints((prev) => [row, ...prev.filter((v) => v.voiceprint_id !== row.voiceprint_id)]);
    return row;
  }, []);

  const deleteVoiceprint = useCallback(async (voiceprintId: string) => {
    await ipcBridge.meeting.deleteVoiceprint.invoke({ voiceprint_id: voiceprintId });
    setVoiceprints((prev) => prev.filter((v) => v.voiceprint_id !== voiceprintId));
  }, []);

  const generateNotes = useCallback(async (sessionId: string) => {
    const result = await ipcBridge.meeting.generateNotes.invoke({ session_id: sessionId });
    setSessions((prev) => upsertSession(prev, result.session));
    return result;
  }, []);

  return {
    sessions,
    selected,
    selectedId,
    selectSession,
    segments,
    segmentsLoading,
    devices,
    detectedApp,
    voiceprints,
    capabilityDegrade,
    loading,
    lastError,
    setLastError,
    createSession,
    startSession,
    pauseSession,
    resumeSession,
    stopSession,
    bindConversation,
    enrollVoiceprint,
    deleteVoiceprint,
    generateNotes,
    refreshVoiceprints,
  };
}
