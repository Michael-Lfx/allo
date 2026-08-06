/**
 * Local-only canvas node -> asset sync for allo (no remote project linking).
 */

import {
  canvasNodeToAsset,
  declaredCanvasNodeAssetCategory,
  findCanvasNodeAsset,
  type CanvasAssetSource,
} from '@oc/lib/canvas/canvas-node-asset';
import { useAssetStore, type AssetCategory } from '@oc/stores/use-asset-store';
import type { CanvasNodeData } from '@oc/types/canvas';

type EnsureCanvasNodeAssetOptions = {
  canvasId: string;
  domainProjectId?: string;
  node: CanvasNodeData;
  source: CanvasAssetSource;
  taskId?: string;
  category?: AssetCategory;
};

export type CanvasNodeAssetResult = {
  assetId: string;
  created: boolean;
  linkedToProject: boolean;
};

const pendingAssetSyncs = new Map<string, Promise<CanvasNodeAssetResult>>();

export function ensureCanvasNodeAsset(options: EnsureCanvasNodeAssetOptions) {
  const identity =
    options.taskId || options.node.metadata?.taskId || options.node.metadata?.storageKey || options.node.id;
  const key = [options.domainProjectId || 'personal', options.canvasId, options.node.id, identity].join(':');
  const pending = pendingAssetSyncs.get(key);
  if (pending) return pending;
  const request = persistCanvasNodeAsset(options).finally(() => pendingAssetSyncs.delete(key));
  pendingAssetSyncs.set(key, request);
  return request;
}

async function persistCanvasNodeAsset(options: EnsureCanvasNodeAssetOptions): Promise<CanvasNodeAssetResult> {
  const store = useAssetStore.getState();
  let asset = findCanvasNodeAsset(store.assets, options.node, options.canvasId, options.taskId);
  const declaredCategory = options.category || declaredCanvasNodeAssetCategory(options.node);
  let created = false;
  if (!asset) {
    const input = canvasNodeToAsset(options.node, {
      canvasId: options.canvasId,
      source: options.source,
      taskId: options.taskId,
    });
    if (!input) throw new Error('Node has no saveable asset content');
    const assetId = store.addAsset(options.category ? { ...input, category: options.category } : input);
    asset = useAssetStore.getState().assets.find((item) => item.id === assetId);
    created = true;
  }
  if (!asset) throw new Error('Failed to write asset locally');
  if (declaredCategory && asset.category !== declaredCategory) {
    store.updateAsset(asset.id, { category: declaredCategory });
    asset = useAssetStore.getState().assets.find((item) => item.id === asset?.id) || asset;
  }

  // Domain project remote linking is disabled in allo; keep local metadata only.
  if (options.domainProjectId) {
    const linkedProjectIds = Array.isArray(asset.metadata?.projectIds)
      ? asset.metadata.projectIds.filter((id): id is string => typeof id === 'string')
      : [];
    useAssetStore.getState().updateAsset(asset.id, {
      category: (declaredCategory || asset.category) as AssetCategory,
      metadata: {
        ...asset.metadata,
        projectIds: [...new Set([...linkedProjectIds, options.domainProjectId])],
      },
    });
  }

  return { assetId: asset.id, created, linkedToProject: false };
}
