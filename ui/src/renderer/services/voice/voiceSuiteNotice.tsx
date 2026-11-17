import React from 'react';
import i18n from 'i18next';
import { Button } from '@arco-design/web-react';
import { appNotifications, type NotificationHandle } from '@/renderer/components/notifications';

/**
 * One place that decides how a voice-suite problem is announced.
 *
 * Entry points (home voice button, conversation composer, desktop companion)
 * used to write their own toast copy and their own settings navigation. They
 * now hand the failure to this module so the same reason always produces the
 * same message and the same way out. The renderer is the existing global
 * notification facade; nothing new is mounted in the main window.
 */
export type VoiceSuiteNoticeKind =
  | 'setup-required'
  | 'not-installed'
  | 'not-ready'
  | 'prepare-failed'
  | 'start-failed'
  | 'unavailable';

export interface VoiceSuiteNoticeInput {
  kind: VoiceSuiteNoticeKind;
  reasonCode?: string | null;
  /** Optional caller-side detail (for example the host error message). */
  detail?: string | null;
  degradedTextOnly?: boolean;
  /** Navigation is owned by the caller: it knows the window and the router. */
  onOpenSettings?: () => void;
}

export type VoiceSuiteNoticeLevel = 'warning' | 'error';

export interface VoiceSuiteNoticeCopy {
  level: VoiceSuiteNoticeLevel;
  title: string;
  content: string;
  announce: string;
}

export const VOICE_SUITE_NOTICE_ID = 'voice-suite-notice';

/** Notices are guidance, not transient status: 8s is enough to read and act. */
export const VOICE_SUITE_NOTICE_DURATION_MS = 8000;

export type VoiceSuiteNoticeTranslator = (
  key: string,
  options?: { defaultValue?: string },
) => string;

// The app entry (`renderer/main.tsx`) initializes the i18next default instance
// before any page renders, so reading it here is safe and keeps this module free
// of a circular dependency on the i18n service module.
const defaultTranslate: VoiceSuiteNoticeTranslator = (key, options) =>
  i18n.t(key, options as never) as unknown as string;

const reasonText = (translate: VoiceSuiteNoticeTranslator, reasonCode?: string | null): string => {
  if (!reasonCode) return '';
  return translate(`voice.capabilities.reason.${reasonCode}`, { defaultValue: reasonCode });
};

const withReason = (
  translate: VoiceSuiteNoticeTranslator,
  base: string,
  reasonCode?: string | null,
): string => {
  const reason = reasonText(translate, reasonCode);
  return reason ? `${base}: ${reason}` : base;
};

/**
 * Pure mapping from a failure kind to user-visible copy. The translator is
 * injectable so the mapping can be tested without booting the i18n runtime.
 */
export function buildVoiceSuiteNotice(
  input: VoiceSuiteNoticeInput,
  translate: VoiceSuiteNoticeTranslator = defaultTranslate,
): VoiceSuiteNoticeCopy {
  const t = translate;
  const announce = (title: string, content: string): string =>
    content && content !== title ? `${title}: ${content}` : content || title;

  switch (input.kind) {
    case 'setup-required': {
      const title = t('voice.entry.setup');
      const content = t('voice.entry.setupRequired');
      return { level: 'warning', title, content, announce: announce(title, content) };
    }
    case 'not-installed': {
      const title = t('voice.entry.setup');
      const content = t('voice.errors.modelMissing');
      return { level: 'warning', title, content, announce: announce(title, content) };
    }
    case 'not-ready': {
      const title = t('voice.capabilities.notReadyTitle');
      const content = [
        withReason(t, t('voice.capabilities.notReady'), input.reasonCode),
        input.degradedTextOnly ? t('voice.capabilities.textOnly') : '',
      ]
        .filter(Boolean)
        .join(' ');
      return { level: 'warning', title, content, announce: announce(title, content) };
    }
    case 'prepare-failed': {
      const title = t('voice.capabilities.notReadyTitle');
      const detail = input.detail?.trim();
      const content = withReason(
        t,
        detail || t('voice.capabilities.checkFailed'),
        input.reasonCode,
      );
      return { level: 'error', title, content, announce: announce(title, content) };
    }
    case 'unavailable': {
      const title = t('voice.session.unavailable');
      return { level: 'warning', title, content: title, announce: title };
    }
    case 'start-failed':
    default: {
      const title = t('voice.session.startFailed');
      return { level: 'error', title, content: title, announce: title };
    }
  }
}

/**
 * Shows (or updates) the single voice-suite notice. A stable id makes repeated
 * failures from different entry points update one card instead of stacking.
 */
export function showVoiceSuiteNotice(input: VoiceSuiteNoticeInput): NotificationHandle {
  const copy = buildVoiceSuiteNotice(input);
  let handle: NotificationHandle | undefined;
  handle = appNotifications.show({
    id: VOICE_SUITE_NOTICE_ID,
    level: copy.level,
    title: copy.title,
    content: copy.content,
    announce: copy.announce,
    duration: VOICE_SUITE_NOTICE_DURATION_MS,
    closable: true,
    action: input.onOpenSettings ? (
      <Button
        size='mini'
        type='primary'
        onClick={() => {
          handle?.dismiss();
          input.onOpenSettings?.();
        }}
      >
        {i18n.t('voice.entry.setup')}
      </Button>
    ) : undefined,
  });
  return handle;
}
