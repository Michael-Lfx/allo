/**
 * Banner + explicit write-back for Canvas projects materialized from Montage Agent.
 */

import React, { useCallback, useMemo, useState } from 'react';
import { Button, Message } from '@arco-design/web-react';
import { useNavigate } from 'react-router-dom';
import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';
import { syncProjectFromCanvas } from '@renderer/pages/videoGeneration/api';
import { readAlloCreative } from './alloVimaxBridge';
import { syncCanvasProjectToServer } from './ocBridge';

type Props = {
  projectId: string;
};

type AlloCreativeMontage = {
  source?: string;
  montageProjectId?: string;
  sessionId?: string;
  writeBack?: {
    enabled?: boolean;
    sessionId?: string;
    montageProjectId?: string;
  };
};

export default function VimaxProvenanceBar({ projectId }: Props) {
  const navigate = useNavigate();
  const project = useCanvasStore((s) => s.projects.find((p) => p.id === projectId));
  const allo = useMemo(() => readAlloCreative(project) as AlloCreativeMontage | null, [project]);
  const [syncing, setSyncing] = useState(false);
  const montageProjectId =
    allo?.montageProjectId ||
    allo?.writeBack?.montageProjectId ||
    allo?.sessionId ||
    allo?.writeBack?.sessionId ||
    '';
  const visible = Boolean(
    montageProjectId && (allo?.source === 'nomifun-montage' || allo?.writeBack?.enabled)
  );

  const handleSync = useCallback(async () => {
    if (!montageProjectId || syncing) return;
    setSyncing(true);
    try {
      await syncCanvasProjectToServer(projectId);
      const result = await syncProjectFromCanvas(montageProjectId, {
        project_id: projectId,
      });
      const warn =
        result.warnings?.length > 0
          ? `（${result.warnings.slice(0, 2).join('；')}）`
          : '';
      Message.success(`已写回 Agent 工程：更新 ${result.updated_shots} 镜${warn}`);
    } catch (e) {
      Message.error(`写回失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSyncing(false);
    }
  }, [montageProjectId, projectId, syncing]);

  if (!visible) return null;

  return (
    <div
      style={{
        position: 'absolute',
        top: 56,
        left: '50%',
        transform: 'translateX(-50%)',
        zIndex: 40,
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        maxWidth: 'min(720px, calc(100% - 48px))',
        padding: '8px 12px',
        borderRadius: 10,
        background: 'rgba(15, 23, 42, 0.78)',
        color: '#f8fafc',
        fontSize: 12,
        lineHeight: 1.4,
        backdropFilter: 'blur(8px)',
      }}
    >
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ fontWeight: 600 }}>来自 Agent 工程 · Montage 物化</div>
        <div style={{ opacity: 0.85, marginTop: 2 }}>
          project {montageProjectId.slice(0, 8)}…
          · 写回为显式操作，不会静默覆盖
        </div>
      </div>
      <Button
        size='mini'
        type='outline'
        style={{ color: '#fff', borderColor: 'rgba(255,255,255,0.35)' }}
        onClick={() => navigate(`/video-generation/${encodeURIComponent(montageProjectId)}`)}
      >
        打开 Agent
      </Button>
      <Button size='mini' type='primary' loading={syncing} onClick={() => void handleSync()}>
        写回 Agent
      </Button>
    </div>
  );
}
