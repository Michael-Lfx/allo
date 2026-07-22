/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';

/**
 * Desktop OS notifications when a requirement reaches a terminal AutoWork
 * status (done / failed / needs_review). Complements webhook CompletionNotifier.
 */
export const useAutoWorkDesktopNotify = () => {
  const { t } = useTranslation();

  useEffect(() => {
    return ipcBridge.requirements.onStatusChanged.on((req) => {
      if (req.status !== 'done' && req.status !== 'failed' && req.status !== 'needs_review') {
        return;
      }
      const title =
        req.status === 'done'
          ? t('requirements.notify.doneTitle', { defaultValue: 'Requirement completed' })
          : req.status === 'failed'
            ? t('requirements.notify.failedTitle', { defaultValue: 'Requirement failed' })
            : t('requirements.notify.needsReviewTitle', { defaultValue: 'Requirement needs review' });
      const body = req.title
        ? `${req.tag ? `[${req.tag}] ` : ''}${req.title}`
        : String(req.id);
      void ipcBridge.notification.show.invoke({ title, body }).catch(() => {
        /* notification permission / unsupported host — ignore */
      });
    });
  }, [t]);
};
