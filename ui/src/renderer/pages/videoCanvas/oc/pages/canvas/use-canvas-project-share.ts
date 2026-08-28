import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { App } from 'antd';

import { isInvalidCloudSessionError, BackendRequestError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { isDesktopShell } from '@renderer/utils/platform';
import { canvasT } from '@oc/lib/canvas/canvas-i18n';
import { resourceIdFromStorageKey } from '@oc/services/api/resources';
import { extractMediaIdFromCanvasMediaUrl } from '../../../api';
import {
  exportCanvasProjectToDisk,
  publishCanvasProject,
} from '../../../lib/canvasProjectShare';
import type { CanvasProject } from '@oc/stores/canvas/use-canvas-store';

function hasCanvasImage(project: CanvasProject | undefined): boolean {
  if (!project) return false;
  return project.nodes.some((node) => {
    if (node.type !== 'image' && node.type !== 'drawing') return false;
    return Boolean(
      node.metadata?.mediaId ||
        resourceIdFromStorageKey(node.metadata?.storageKey) ||
        extractMediaIdFromCanvasMediaUrl(node.metadata?.content) ||
        node.metadata?.storageKey ||
        node.metadata?.content ||
        (node.type === 'drawing' && node.metadata?.drawingId)
    );
  });
}

function coverRequiredMessage(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  return /cover image is required/i.test(text);
}

function publishTimedOut(error: unknown): boolean {
  if (error instanceof BackendRequestError && error.kind === 'timeout') return true;
  const text = error instanceof Error ? error.message : String(error);
  return /timed out/i.test(text);
}

export function useCanvasProjectShare(project: CanvasProject | undefined) {
  const { message } = App.useApp();
  const navigate = useNavigate();
  const { status: cloudStatus, logout } = useCloudAuth();
  const [exporting, setExporting] = useState(false);
  const [publishing, setPublishing] = useState(false);

  const exportProject = useCallback(async () => {
    if (exporting || !project) return;
    if (!isDesktopShell()) {
      message.info(canvasT('videoCanvas.share.exportDesktopOnly', '导出工程仅桌面端可用。'));
      return;
    }
    setExporting(true);
    try {
      const dest = await exportCanvasProjectToDisk(project.id, project.title);
      if (!dest) return;
      message.success(
        canvasT('videoCanvas.share.exportOk', '工程已导出：{{path}}', { path: dest })
      );
    } catch (error) {
      message.error(
        `${canvasT('videoCanvas.share.exportFailed', '导出失败')}: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    } finally {
      setExporting(false);
    }
  }, [exporting, message, project]);

  const publishToTvShow = useCallback(async () => {
    if (publishing || !project) return;
    if (cloudStatus !== 'authenticated') {
      message.warning(
        canvasT('videoCanvas.share.authRequired', '发布到 Flowy TV 需要先登录云端账号。')
      );
      navigate('/cloud-login');
      return;
    }
    if (!hasCanvasImage(project)) {
      message.warning(
        canvasT(
          'videoCanvas.share.needCover',
          '发布需要封面图。请在图片节点上右键「设为封面」，或至少在画布上放一张图片以便自动选择。'
        )
      );
      return;
    }
    setPublishing(true);
    const hide = message.loading(
      canvasT(
        'videoCanvas.share.publishing',
        '正在上传工程包到 Flowy TV，文件较大时可能需要几分钟，请保持网络畅通。'
      ),
      0
    );
    try {
      await publishCanvasProject(project.id, project.title);
      message.success(
        canvasT('videoCanvas.share.publishOk', '已提交审核，通过后会出现在 Flowy TV 广场。')
      );
    } catch (error) {
      if (isInvalidCloudSessionError(error)) {
        await logout();
        navigate('/cloud-login');
        return;
      }
      if (coverRequiredMessage(error)) {
        message.warning(
          canvasT(
            'videoCanvas.share.needCover',
            '发布需要封面图。请在图片节点上右键「设为封面」，或至少在画布上放一张图片以便自动选择。'
          )
        );
        return;
      }
      if (publishTimedOut(error)) {
        message.error(
          canvasT(
            'videoCanvas.share.publishTimeout',
            '上传超时。请检查网络后重试；工程包较大时请稍后再试。'
          )
        );
        return;
      }
      message.error(
        `${canvasT('videoCanvas.share.publishFailed', '发布失败')}: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    } finally {
      hide();
      setPublishing(false);
    }
  }, [cloudStatus, logout, message, navigate, project, publishing]);

  return { exporting, publishing, exportProject, publishToTvShow };
}
