import { useCallback, useEffect, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import type {
  MeetingDevice,
  MeetingEvent,
  MeetingListenStatus,
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

export type UseMeetingsOptions = {
  sessionId?: string | null;
  autoSelectFirst?: boolean;
};

export function useMeetings(options: UseMeetingsOptions = {}) {
  const autoSelectFirst = options.autoSelectFirst ?? false;
  const preferredId = options.sessionId ?? null;

  const [sessions, setSessions] = useState<MeetingSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(preferredId);
  const [segments, setSegments] = useState<MeetingSegment[]>([]);
  const [devices, setDevices] = useState<MeetingDevice[]>([]);
  const [detectedApp, setDetectedApp] = useState<string | null>(null);
  const [voiceprints, setVoiceprints] = useState<MeetingVoiceprint[]>([]);
  const [capabilityDegrade, setCapabilityDegrade] = useState<CapabilityDegrade>(null);
  const [listenStatus, setListenStatus] = useState<MeetingListenStatus | null>(null);
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

  const loadListenStatus = useCallback(async (sessionId: string) => {
    try {
      const status = await ipcBridge.meeting.listenStatus.invoke({ session_id: sessionId });
      setListenStatus(status);
    } catch {
      setListenStatus(null);
    }
  }, []);

  const selectSession = useCallback(
    (sessionId: string | null) => {
      setSelectedId(sessionId);
      setCapabilityDegrade((prev) => (prev && prev.session_id === sessionId ? prev : null));
      if (sessionId) {
        void loadSegments(sessionId);
        void loadListenStatus(sessionId);
      } else {
        setSegments([]);
        setListenStatus(null);
      }
    },
    [loadListenStatus, loadSegments]
  );

  const preferredIdRef = useRef(preferredId);
  preferredIdRef.current = preferredId;

  const bootstrap = useCallback(async () => {
    setLoading(true);
    try {
      const list = await refreshSessions();
      await Promise.all([refreshDevices(), refreshDetectedApps(), refreshVoiceprints()]);
      const currentPreferredId = preferredIdRef.current;
      let nextId =
        currentPreferredId && list.some((item) => item.session_id === currentPreferredId)
          ? currentPreferredId
          : null;
      if (currentPreferredId && !nextId) {
        try {
          const remote = await ipcBridge.meeting.getSession.invoke({ session_id: currentPreferredId });
          if (remote) {
            setSessions((prev) => upsertSession(prev, remote));
            nextId = remote.session_id;
          }
        } catch {
          nextId = null;
        }
      }
      if (!nextId && autoSelectFirst) {
        nextId = list[0]?.session_id ?? null;
      }
      if (nextId) {
        setSelectedId(nextId);
        await loadSegments(nextId);
        await loadListenStatus(nextId);
      } else {
        setSelectedId(currentPreferredId);
        setSegments([]);
        setListenStatus(null);
      }
    } finally {
      setLoading(false);
    }
  }, [
    autoSelectFirst,
    loadListenStatus,
    loadSegments,
    refreshDetectedApps,
    refreshDevices,
    refreshSessions,
    refreshVoiceprints,
  ]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  useEffect(() => {
    if (preferredId && preferredId !== selectedId) {
      selectSession(preferredId);
    }
  }, [preferredId, selectSession, selectedId]);

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
  }, [loadSegments, refreshDetectedApps, refreshDevices, refreshSessions, selectedId]);

  const createSession = useCallback(
    async (params: {
      title: string;
      stt_backend?: SttBackendChoice;
      bound_conversation_id?: string | null;
    }) => {
      const session = await ipcBridge.meeting.createSession.invoke({
        title: params.title.trim() || undefined,
        stt_backend: params.stt_backend ?? 'auto',
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

  const updateTitle = useCallback(async (sessionId: string, title: string) => {
    const session = await ipcBridge.meeting.updateSession.invoke({
      session_id: sessionId,
      title,
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

  const editSegment = useCallback(async (sessionId: string, segmentId: string, text: string) => {
    const segment = await ipcBridge.meeting.editSegment.invoke({
      session_id: sessionId,
      segment_id: segmentId,
      text,
    });
    setSegments((prev) => upsertSegment(prev, segment));
    return segment;
  }, []);

  const searchSegments = useCallback(async (sessionId: string, q: string) => {
    if (!q.trim()) {
      await loadSegments(sessionId);
      return;
    }
    setSegmentsLoading(true);
    try {
      const list = await ipcBridge.meeting.searchSegments.invoke({
        session_id: sessionId,
        q,
        limit: 200,
      });
      setSegments((list ?? []).slice().sort((a, b) => a.start_ms - b.start_ms || a.end_ms - b.end_ms));
    } catch {
      setSegments([]);
    } finally {
      setSegmentsLoading(false);
    }
  }, [loadSegments]);

  const startListen = useCallback(async (sessionId: string, conversationId?: string | null) => {
    const status = await ipcBridge.meeting.listenStart.invoke({
      session_id: sessionId,
      conversation_id: conversationId,
    });
    setListenStatus(status);
    if (status.conversation_id) {
      setSessions((prev) =>
        prev.map((session) =>
          session.session_id === sessionId
            ? { ...session, bound_conversation_id: status.conversation_id }
            : session
        )
      );
    }
    return status;
  }, []);

  const stopListen = useCallback(async (sessionId: string) => {
    const status = await ipcBridge.meeting.listenStop.invoke({ session_id: sessionId });
    setListenStatus(status);
    return status;
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
    listenStatus,
    loading,
    lastError,
    setLastError,
    createSession,
    startSession,
    pauseSession,
    resumeSession,
    stopSession,
    bindConversation,
    updateTitle,
    enrollVoiceprint,
    deleteVoiceprint,
    generateNotes,
    editSegment,
    searchSegments,
    startListen,
    stopListen,
    refreshVoiceprints,
    refreshDetectedApps,
  };
}
