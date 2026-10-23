import React, { useCallback, useEffect, useState } from 'react';
import { Button } from '@arco-design/web-react';
import { QRCodeSVG } from 'qrcode.react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { link } from '@/common/adapter/ipcBridge';

type PendingPhone = {
  nonce: string;
  displayName: string;
  platform: string;
  appVersion: string;
};

type PairingSession = {
  nonce: string;
  expiresAtMs: number;
  url?: string | null;
  fingerprint?: string;
};

export default function LinkPairingPanel() {
  const [session, setSession] = useState<PairingSession | null>(null);
  const [pending, setPending] = useState<PendingPhone[]>([]);
  const [confirmations, setConfirmations] = useState<Array<{ id: string; method: string; path: string }>>([]);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  const refresh = useCallback(async () => {
    const [phones, confirmations] = await Promise.all([link.pending.invoke(), link.stepUpPending.invoke()]);
    setPending(phones?.items ?? []);
    setConfirmations(confirmations?.items ?? []);
  }, []);

  const open = async () => {
    setOpening(true);
    setError(null);
    try {
      const opened = await link.openPairing.invoke();
      if (!opened?.url) {
        setSession(null);
        setError('这台电脑还没有可被手机校验证书的地址。确认 Link 监听已启动，并且网卡有局域网地址。');
        return;
      }
      setSession(opened);
      await refresh();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : '生成配对码失败';
      setError(message);
      Message.error(message);
    } finally {
      setOpening(false);
    }
  };

  // useEffect必要性：桌面壳轮询本机待配对手机和二次确认；目的：手机请求后立刻出现允许按钮。未使用 ahooks：allo 界面工程没有 ahooks。
  useEffect(() => {
    const pull = () => {
      void refresh().catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : '读取手机请求失败');
      });
    };
    pull();
    const timer = window.setInterval(pull, 2000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const decide = async (nonce: string, allow: boolean) => {
    setError(null);
    try {
      await link.decide.invoke({ nonce, allow });
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : '处理配对请求失败');
    }
  };

  const loopbackOnly = Boolean(session?.url?.includes('h=127.0.0.1') && !session.url.includes(','));

  return (
    <div className='flex flex-col gap-12px'>
      <Button type='primary' loading={opening} onClick={() => void open()} style={{ minHeight: 44 }}>
        生成配对码
      </Button>
      {error ? <div className='text-13px text-danger-6'>{error}</div> : null}
      {session?.url ? (
        <div className='flex flex-col items-start gap-12px'>
          <QRCodeSVG value={session.url} size={188} />
          <div className='text-13px text-t-secondary' style={{ fontVariantNumeric: 'tabular-nums' }}>
            证书 {session.fingerprint}
          </div>
          {loopbackOnly ? <div className='text-13px text-danger-6'>当前只有本机地址，同一局域网里的手机扫不到这张码。</div> : null}
          <div className='max-w-full break-all text-12px text-t-tertiary' style={{ fontVariantNumeric: 'tabular-nums' }}>
            {session.url}
          </div>
        </div>
      ) : null}
      {confirmations.map((item) => (
        <div key={item.id} className='rd-12px border border-solid border-arco-2 p-12px'>
          <div className='text-14px'>手机请求确认</div>
          <div className='text-12px text-t-tertiary' style={{ fontVariantNumeric: 'tabular-nums' }}>
            {item.method} {item.path}
          </div>
          <div className='mt-8px flex gap-8px'>
            <Button type='primary' onClick={() => void link.stepUpDecide.invoke({ id: item.id, allow: true }).then(() => refresh())} style={{ minHeight: 44 }}>
              允许这次
            </Button>
            <Button onClick={() => void link.stepUpDecide.invoke({ id: item.id, allow: false }).then(() => refresh())} style={{ minHeight: 44 }}>
              拒绝
            </Button>
          </div>
        </div>
      ))}
      {pending.map((phone) => (
        <div key={phone.nonce} className='rd-12px border border-solid border-arco-2 p-12px'>
          <div className='text-14px'>{phone.displayName}</div>
          <div className='text-12px text-t-tertiary'>
            {phone.platform}
            {phone.appVersion ? ` · ${phone.appVersion}` : ''}
          </div>
          <div className='mt-8px flex gap-8px'>
            <Button type='primary' onClick={() => void decide(phone.nonce, true)} style={{ minHeight: 44 }}>
              允许
            </Button>
            <Button onClick={() => void decide(phone.nonce, false)} style={{ minHeight: 44 }}>
              拒绝
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}
