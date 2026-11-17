import { describe, expect, test } from 'bun:test';
import { appNotifications } from '@/renderer/components/notifications';
import {
  VOICE_SUITE_NOTICE_DURATION_MS,
  VOICE_SUITE_NOTICE_ID,
  buildVoiceSuiteNotice,
  showVoiceSuiteNotice,
} from './voiceSuiteNotice';

const dictionary: Record<string, string> = {
  'voice.entry.setup': '设置语音',
  'voice.entry.setupRequired': '语音模型缺失、未校验或语音已停用。请前往语音设置下载、校验或启用。',
  'voice.errors.modelMissing': '语音模型套件尚未安装，请先在语音设置下载并校验。',
  'voice.capabilities.notReadyTitle': '完整通话未就绪',
  'voice.capabilities.notReady': '完整通话尚不可用；文字回复仍可继续使用。',
  'voice.capabilities.checkFailed': '就绪检查失败',
  'voice.capabilities.textOnly': '当前仍可使用文字回复。',
  'voice.capabilities.reason.self_check_required': '需要先运行就绪检查',
  'voice.capabilities.reason.asr_inference_failed': '语音识别推理失败',
  'voice.session.startFailed': '启动语音通话失败',
  'voice.session.unavailable': '语音通话暂不可用',
};

const translate = (key: string, options?: { defaultValue?: string }): string =>
  dictionary[key] ?? options?.defaultValue ?? key;

describe('unified voice-suite notice copy', () => {
  test('setup problems explain the missing pack and point at settings', () => {
    const copy = buildVoiceSuiteNotice({ kind: 'setup-required' }, translate);
    expect(copy.level).toBe('warning');
    expect(copy.title).toBe('设置语音');
    expect(copy.content).toBe(dictionary['voice.entry.setupRequired']);
    expect(copy.announce).toContain(copy.title);
  });

  test('a not-installed failure uses the model-missing wording', () => {
    const copy = buildVoiceSuiteNotice({ kind: 'not-installed' }, translate);
    expect(copy.level).toBe('warning');
    expect(copy.content).toBe(dictionary['voice.errors.modelMissing']);
  });

  test('not-ready keeps the reason code and the text-only fallback', () => {
    const copy = buildVoiceSuiteNotice(
      { kind: 'not-ready', reasonCode: 'asr_inference_failed', degradedTextOnly: true },
      translate,
    );
    expect(copy.level).toBe('warning');
    expect(copy.title).toBe(dictionary['voice.capabilities.notReadyTitle']);
    expect(copy.content).toContain(dictionary['voice.capabilities.notReady']);
    expect(copy.content).toContain(dictionary['voice.capabilities.reason.asr_inference_failed']);
    expect(copy.content).toContain(dictionary['voice.capabilities.textOnly']);
  });

  test('an unknown reason code falls back to the raw code instead of a blank', () => {
    const copy = buildVoiceSuiteNotice({ kind: 'not-ready', reasonCode: 'mystery_failure' }, translate);
    expect(copy.content).toContain('mystery_failure');
  });

  test('prepare failures keep a caller-supplied detail when one exists', () => {
    const copy = buildVoiceSuiteNotice(
      { kind: 'prepare-failed', detail: '语音服务已停用', reasonCode: 'asr_inference_failed' },
      translate,
    );
    expect(copy.content).toContain('语音服务已停用');
    expect(copy.content).toContain(dictionary['voice.capabilities.reason.asr_inference_failed']);
  });

  test('prepare and start failures are errors, not warnings', () => {
    expect(buildVoiceSuiteNotice({ kind: 'prepare-failed', reasonCode: 'asr_inference_failed' }, translate).level).toBe('error');
    expect(buildVoiceSuiteNotice({ kind: 'start-failed' }, translate).level).toBe('error');
    expect(buildVoiceSuiteNotice({ kind: 'unavailable' }, translate).content).toBe(dictionary['voice.session.unavailable']);
  });
});

describe('unified voice-suite notice rendering', () => {
  const originalShow = appNotifications.show;

  const installSpy = () => {
    const shown: Array<Record<string, unknown>> = [];
    const dismissed: number[] = [];
    (appNotifications as unknown as { show: typeof appNotifications.show }).show = ((
      input: Record<string, unknown>,
    ) => {
      shown.push(input);
      return {
        id: 'spy',
        dismiss: () => dismissed.push(1),
        update: () => undefined,
      };
    }) as typeof appNotifications.show;
    return { shown, dismissed };
  };

  const restore = () => {
    (appNotifications as unknown as { show: typeof appNotifications.show }).show = originalShow;
  };

  test('repeated failures update one card instead of stacking', () => {
    const { shown } = installSpy();
    try {
      showVoiceSuiteNotice({ kind: 'not-ready', reasonCode: 'asr_inference_failed' });
      showVoiceSuiteNotice({ kind: 'not-ready', reasonCode: 'kws_inference_failed' });
    } finally {
      restore();
    }
    expect(shown).toHaveLength(2);
    expect(shown[0].id).toBe(VOICE_SUITE_NOTICE_ID);
    expect(shown[1].id).toBe(VOICE_SUITE_NOTICE_ID);
    expect(shown[0].duration).toBe(VOICE_SUITE_NOTICE_DURATION_MS);
    expect(shown[0].closable).toBe(true);
  });

  test('the action button navigates only when the caller owns navigation, and dismisses first', () => {
    const { shown, dismissed } = installSpy();
    let opened = 0;
    try {
      showVoiceSuiteNotice({ kind: 'not-ready', onOpenSettings: () => (opened += 1) });
      showVoiceSuiteNotice({ kind: 'start-failed' });
    } finally {
      restore();
    }
    const action = shown[0].action as { props: { onClick: () => void } } | undefined;
    expect(action).toBeTruthy();
    expect(shown[1].action).toBeUndefined();
    action!.props.onClick();
    expect(dismissed).toHaveLength(1);
    expect(opened).toBe(1);
  });
});
