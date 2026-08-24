import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import './completionToast.css';

type CompletionToastPayload = {
  generation: number;
  title: string;
  body: string;
  click_target?: string | null;
};

const CompletionToastPage: React.FC = () => {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<CompletionToastPayload | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<CompletionToastPayload>('completion-toast://show', (event) => {
        if (disposed) return;
        setPayload(event.payload);
      });
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const invokeToastCommand = useCallback(async (cmd: string, generation: number) => {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke(cmd, { generation });
  }, []);

  const onOpen = useCallback(() => {
    if (!payload) return;
    void invokeToastCommand('activate_completion_toast', payload.generation).finally(() => {
      setPayload(null);
    });
  }, [invokeToastCommand, payload]);

  const onDismiss = useCallback(() => {
    if (!payload) return;
    void invokeToastCommand('dismiss_completion_toast', payload.generation).finally(() => {
      setPayload(null);
    });
  }, [invokeToastCommand, payload]);

  if (!payload) {
    return <main className='completion-toast completion-toast--empty' />;
  }

  return (
    <main className='completion-toast'>
      <section className='completion-toast__card' aria-live='polite'>
        <button type='button' className='completion-toast__content' onClick={onOpen}>
          <span className='completion-toast__title'>{payload.title}</span>
          <span className='completion-toast__body'>{payload.body}</span>
        </button>
        <div className='completion-toast__actions'>
          <button type='button' className='completion-toast__action completion-toast__action--primary' onClick={onOpen}>
            {t('conversation.notify.open', { defaultValue: '打开' })}
          </button>
          <button type='button' className='completion-toast__action' onClick={onDismiss}>
            {t('conversation.notify.dismiss', { defaultValue: '关闭' })}
          </button>
        </div>
      </section>
    </main>
  );
};

export default CompletionToastPage;
