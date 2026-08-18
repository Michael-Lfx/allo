/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { createElement, useCallback, useEffect, useState } from 'react';
import { Message, Notification } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type {
  IKnowledgeBase,
  IKnowledgeConsumer,
  IKnowledgeFileEntry,
  IKnowledgeSource,
  IKnowledgeSourceFetchSummary,
  IKnowledgeTreeEntry,
} from '@/common/adapter/ipcBridge';
import type { I18nKey } from '@/renderer/services/i18n';
import type { KnowledgeBaseId } from '@/common/types/ids';

export function patchKnowledgeBase(bases: IKnowledgeBase[], next: IKnowledgeBase): IKnowledgeBase[] {
  const index = bases.findIndex((base) => base.knowledge_base_id === next.knowledge_base_id);
  if (index < 0) return [...bases, next];
  const nextBases = bases.slice();
  nextBases[index] = next;
  return nextBases;
}

export function removeKnowledgeBase(bases: IKnowledgeBase[], id: KnowledgeBaseId): IKnowledgeBase[] {
  return bases.filter((base) => base.knowledge_base_id !== id);
}

export function useKnowledgeBases(options?: { enabled?: boolean }) {
  const enabled = options?.enabled ?? true;
  const [bases, setBases] = useState<IKnowledgeBase[]>([]);
  const [loading, setLoading] = useState(enabled);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const res = await ipcBridge.knowledge.listBases.invoke();
      setBases(res);
      setError(null);
    } catch (e) {
      console.error('Failed to load knowledge bases', e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      setLoading(false);
      return;
    }
    void refresh();
  }, [enabled, refresh]);

  useEffect(() => {
    if (!enabled) return;
    const unsubs = [
      ipcBridge.knowledge.onBaseCreated.on((base) => {
        setBases((current) => patchKnowledgeBase(current, base));
      }),
      ipcBridge.knowledge.onBaseUpdated.on((base) => {
        setBases((current) => patchKnowledgeBase(current, base));
      }),
      ipcBridge.knowledge.onBaseDeleted.on((event) => {
        setBases((current) => removeKnowledgeBase(current, event.knowledge_base_id));
      }),
    ];
    return () => unsubs.forEach((u) => u());
  }, [enabled]);

  return { bases, loading, error, refresh };
}

export function useKnowledgeBase(id: KnowledgeBaseId | undefined) {
  const [base, setBase] = useState<IKnowledgeBase | null>(null);
  const [files, setFiles] = useState<IKnowledgeFileEntry[]>([]);
  const [tree, setTree] = useState<IKnowledgeTreeEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!id) return;
    setLoading(true);
    try {
      const [info, treeRoot] = await Promise.all([
        ipcBridge.knowledge.getBase.invoke({ knowledge_base_id: id }),
        ipcBridge.knowledge.listTree.invoke({ knowledge_base_id: id }),
      ]);
      setBase(info);
      setTree(treeRoot);
      setError(null);
    } catch (e) {
      console.error('Failed to load knowledge base', e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [id]);

  const loadFiles = useCallback(async () => {
    if (!id) return [];
    const list = await ipcBridge.knowledge.listFiles.invoke({ knowledge_base_id: id });
    setFiles(list);
    return list;
  }, [id]);

  useEffect(() => {
    setFiles([]);
    setTree([]);
    setBase(null);
    void refresh();
  }, [refresh]);

  // Stats refresh broadcasts knowledge.base-updated — patch the header only.
  // Reloading tree/files here would re-walk the vault on every background count.
  useEffect(() => {
    if (!id) return;
    const unsub = ipcBridge.knowledge.onBaseUpdated.on((updated) => {
      if (updated.knowledge_base_id === id) setBase(updated);
    });
    return () => unsub();
  }, [id]);

  return { base, files, tree, loading, error, refresh, loadFiles };
}

/** Bindings (workspaces/conversations/…) currently mounting a base. */
export function useKnowledgeConsumers(id: KnowledgeBaseId | undefined) {
  const [consumers, setConsumers] = useState<IKnowledgeConsumer[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!id) return;
    setLoading(true);
    try {
      const res = await ipcBridge.knowledge.listConsumers.invoke({ knowledge_base_id: id });
      setConsumers(res);
      setError(null);
    } catch (e) {
      console.error('Failed to load knowledge consumers', e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const unsub = ipcBridge.knowledge.onBindingChanged.on(() => void refresh());
    return () => unsub();
  }, [refresh]);

  return { consumers, loading, error, refresh };
}

/** Null-safe accessor for a base's URL source config (top-level `source` on the wire). */
export function getBaseSource(base: IKnowledgeBase | null | undefined): IKnowledgeSource | undefined {
  return base?.source;
}

/** Human-readable message for a knowledge API failure (prefers the backend-provided message). */
export function knowledgeErrorText(e: unknown): string {
  if (isBackendHttpError(e) && e.backendMessage.trim()) return e.backendMessage;
  return e instanceof Error ? e.message : String(e);
}

/** True when the error is the autogen 409 — no AI completer/provider configured. */
export function isAutogenNoProviderError(e: unknown): boolean {
  return isBackendHttpError(e) && e.status === 409;
}

type TranslateFn = (key: I18nKey, options?: Record<string, unknown>) => string;

/**
 * Surface a URL-source fetch outcome (create-time `source_fetch` / refresh-source
 * response). Failures get a sticky notification listing each failed URL; a fully
 * successful run shows `okMessage` when provided (callers pass none at create
 * time, where the regular "created" toast already covers it).
 */
export function notifySourceFetchResult(t: TranslateFn, summary: IKnowledgeSourceFetchSummary, okMessage?: string): void {
  if (summary.failed > 0) {
    Notification.warning({
      title: t('knowledge.source.fetchFailedTitle'),
      content: createElement(
        'div',
        { className: 'flex flex-col gap-4px max-h-220px overflow-y-auto' },
        createElement(
          'span',
          { key: 'summary' },
          t('knowledge.source.fetchSummary', { fetched: summary.fetched, failed: summary.failed })
        ),
        ...summary.errors.map((line, i) => createElement('span', { key: i, className: 'text-12px break-all' }, line))
      ),
      duration: 10000,
    });
  } else if (okMessage) {
    Message.success(okMessage);
  }
}

/** Render a byte count as a short human-readable size. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
