/**
 * Banner + explicit write-back for Canvas projects materialized from ViMax Agent.
 */

import React, { useCallback, useMemo, useState } from 'react';
import { Button } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useNavigate } from 'react-router-dom';
import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';
import { syncSessionFromCanvas } from '@renderer/pages/videoGeneration/api';
import { readAlloCreative } from './alloVimaxBridge';
import { syncCanvasProjectToServer } from './ocBridge';

type Props = {
  projectId: string;
};

export default function VimaxProvenanceBar({ projectId }: Props) {
  const navigate = useNavigate();
  const project = useCanvasStore((s) => s.projects.find((p) => p.id === projectId));
  const allo = useMemo(() => readAlloCreative(project), [project]);
  const [syncing, setSyncing] = useState(false);
  const sessionId = allo?.sessionId || allo?.writeBack?.sessionId || '';
  const visible = Boolean(sessionId && allo?.writeBack?.enabled);

  const handleSync = useCallback(async () => {
    if (!sessionId || syncing) return;
    setSyncing(true);
    try {
      await syncCanvasProjectToServer(projectId);
      const result = await syncSessionFromCanvas(sessionId, {
        project_id: projectId,
        reconcat: true,
      });
      if (result.updated_shots === 0) {
        Message.info('Canvas 中暂无需要写回的视频更新');
        return;
      }
      const warn =
        result.warnings?.length > 0
          ? `（${result.warnings.slice(0, 2).join('；')}）`
          : '';
      // 区分两种结果：写入成功但 concat 被跳过（部分 shot 还没就绪） vs 全部完成。
      const concatBlocked = result.warnings?.some((w) =>
        w.includes('concat skipped') || w.includes('concat失败')
      );
      if (concatBlocked) {
        Message.warning(`已写回 ${result.updated_shots} 镜，但部分 shot 未生成，成片暂不拼接${warn}`);
      } else {
        Message.success(`已写回 Agent 工程：更新 ${result.updated_shots} 镜${warn}`);
      }
    } catch (e) {
      Message.error(`写回失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSyncing(false);
    }
  }, [projectId, sessionId, syncing]);

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
        <div style={{ fontWeight: 600 }}>来自 Agent 工程 · 高保真物化</div>
        <div style={{ opacity: 0.85, marginTop: 2 }}>
          session {sessionId.slice(0, 8)}…
          {allo?.workflow ? ` · ${allo.workflow}` : ''}
          · 重生成会注入 VoiceProfile；写回为显式操作，不会静默覆盖
        </div>
      </div>
      <Button
        size='mini'
        type='outline'
        style={{ color: '#fff', borderColor: 'rgba(255,255,255,0.35)' }}
        onClick={() => navigate(`/video-generation/${encodeURIComponent(sessionId)}`)}
      >
        打开 Agent
      </Button>
      <Button size='mini' type='primary' loading={syncing} onClick={() => void handleSync()}>
        写回 Agent
      </Button>
    </div>
  );
}
