import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { tauriEvents } from '@/common/adapter/tauriEvents';
import { tauriSystem } from '@/common/adapter/tauriSystem';
import './completionToast.css';

type CompletionToastPayload = {
  generation: number;
  title: string;
  body: string;
  click_target?: string | null;
  attention_id?: string | null;
};

const CompletionToastPage: React.FC = () => {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<CompletionToastPayload | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await tauriEvents.listen<CompletionToastPayload>('completion-toast://show', (payload) => {
        if (disposed) return;
        setPayload(payload);
      });
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const onOpen = useCallback(() => {
    if (!payload) return;
    void tauriSystem.activateCompletionToast(payload.generation).finally(() => {
      setPayload(null);
    });
  }, [payload]);

  const onDismiss = useCallback(() => {
    if (!payload) return;
    void tauriSystem.dismissCompletionToast(payload.generation).finally(() => {
      setPayload(null);
    });
  }, [payload]);

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
