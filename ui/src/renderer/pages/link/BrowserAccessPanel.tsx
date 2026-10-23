import { webui } from '@/common/adapter/ipcBridge';
import { copyText } from '@/renderer/utils/ui/clipboard';
import { useWebuiServer } from '@/renderer/hooks/context/WebuiServerContext';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { buildWebuiQrLoginUrl, isVirtualAdapterUrl, listBrowserScanBaseUrls } from '@/renderer/components/layout/Sider/webuiQrLinks';
import { Button, Select } from '@arco-design/web-react';
import { Copy, Refresh } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import { useTranslation } from 'react-i18next';

const QR_REFRESH_MS = 4 * 60 * 1000;

export default function BrowserAccessPanel() {
  const { t } = useTranslation();
  const { status, starting, lifecycleSupported, accessUrls, start } = useWebuiServer();
  const [token, setToken] = useState<string | null>(null);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [selectedBase, setSelectedBase] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  const bases = useMemo(() => listBrowserScanBaseUrls(status, accessUrls), [status, accessUrls]);
  const selected = selectedBase && bases.includes(selectedBase) ? selectedBase : (bases[0] ?? null);
  const qrUrl = token && selected ? buildWebuiQrLoginUrl(selected, token) : null;
  const webReady = status?.running === true;

  const mint = useCallback(async () => {
    setError(null);
    const latest = await webui.getStatus.invoke();
    const nextBases = listBrowserScanBaseUrls(latest, latest?.networkUrls ?? []);
    if (!latest?.running) {
      setToken(null);
      setError(t('settings.link.browserFailed', { defaultValue: '网页访问没有打开。' }));
      return;
    }
    if (nextBases.length === 0) {
      setToken(null);
      setError(
        t('settings.link.browserNoAddress', {
          defaultValue: '当前没有手机能打开的局域网地址。确认电脑和手机在同一网络，并且系统放行了网页端口。',
        })
      );
      return;
    }
    setSelectedBase((current) => (current && nextBases.includes(current) ? current : nextBases[0]));
    const qrData = await webui.generateQRToken.invoke();
    if (!qrData?.token) {
      setToken(null);
      setError(t('settings.webui.qrGenerateFailed'));
      return;
    }
    setToken(qrData.token);
    setExpiresAt(qrData.expires_at_ms);
  }, [t]);

  const open = async () => {
    setOpening(true);
    setError(null);
    try {
      if (!webReady) {
        await start();
        const latest = await webui.getStatus.invoke();
        if (!latest?.running) {
          setError(t('settings.link.browserFailed', { defaultValue: '网页访问没有打开。' }));
          return;
        }
      }
      await mint();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : t('settings.link.browserFailed', { defaultValue: '网页访问没有打开。' });
      setError(message);
      Message.error(message);
    } finally {
      setOpening(false);
    }
  };

  // useEffect必要性：本机网页监听已经打开，登录码四分钟后失效；目的：保持二维码里的局域网网址仍然能登录。未使用 ahooks：allo 界面工程没有 ahooks。
  useEffect(() => {
    if (!webReady || !lifecycleSupported) {
      setToken(null);
      setExpiresAt(null);
      return;
    }
    const pull = () => {
      void mint().catch((cause: unknown) => {
        setError(cause instanceof Error ? cause.message : t('settings.webui.qrGenerateFailed'));
      });
    };
    pull();
    const timer = window.setInterval(pull, QR_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [webReady, lifecycleSupported, mint, t]);

  const expiresLabel =
    expiresAt == null
      ? null
      : t('settings.webui.qrExpires', {
          time: new Date(expiresAt).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }),
        });

  return (
    <div className='flex flex-col gap-12px'>
      <div>
        <div className='text-16px font-600 text-t-primary'>{t('settings.link.browserTitle', { defaultValue: '用浏览器打开' })}</div>
        <div className='mt-4px text-13px text-t-secondary leading-relaxed'>
          {t('settings.link.browserDesc', {
            defaultValue: '不安装 App。用微信、系统相机或其他扫码工具扫描，手机会打开这台电脑的局域网网页并登录。生成二维码时会打开网页访问。',
          })}
        </div>
      </div>

      {!lifecycleSupported ? (
        <div className='text-13px text-danger-6'>{t('settings.webui.desktopLifecycleUnavailable')}</div>
      ) : (
        <Button type='primary' loading={opening || starting} onClick={() => void open()} style={{ minHeight: 44 }}>
          {t('settings.link.browserAction', { defaultValue: '生成网页二维码' })}
        </Button>
      )}
      {error ? <div className='text-13px text-danger-6'>{error}</div> : null}

      {selected && isVirtualAdapterUrl(selected) ? (
        <div className='text-13px text-danger-6'>
          {t('settings.link.browserVirtualAddress', {
            defaultValue: '这个地址来自虚拟网卡，手机浏览器打不开。请让电脑连上 Wi-Fi，再重新生成二维码。',
          })}
        </div>
      ) : null}

      {bases.length > 1 ? (
        <Select value={selected ?? undefined} onChange={(value) => setSelectedBase(value)}>
          {bases.map((url) => (
            <Select.Option key={url} value={url}>
              {url}
            </Select.Option>
          ))}
        </Select>
      ) : null}

      {qrUrl ? (
        <div className='flex flex-col items-start gap-12px'>
          <div className='bg-white p-12px rd-12px'>
            <QRCodeSVG value={qrUrl} size={188} level='M' />
          </div>
          <div className='text-13px text-t-secondary'>
            {t('settings.link.browserWechat', {
              defaultValue: '如果微信停在自己的页面里打不开，点右上角，用系统浏览器打开。',
            })}
          </div>
          {expiresLabel ? (
            <div className='text-12px text-t-tertiary' style={{ fontVariantNumeric: 'tabular-nums' }}>
              {expiresLabel}
            </div>
          ) : null}
          <div className='flex items-center gap-8px'>
            <button
              type='button'
              className='p-4px bg-transparent border-none text-t-tertiary hover:text-t-primary cursor-pointer'
              aria-label={t('settings.webui.copyQrLink')}
              onClick={() => {
                copyText(qrUrl)
                  .then(() => Message.success(t('common.copySuccess')))
                  .catch((copyError) => console.error('[BrowserAccessPanel] Copy failed:', copyError));
              }}
            >
              <Copy size={15} />
            </button>
            <button
              type='button'
              className='p-4px bg-transparent border-none text-t-tertiary hover:text-t-primary cursor-pointer'
              aria-label={t('settings.webui.refreshQr')}
              onClick={() => void open()}
            >
              <Refresh size={15} />
            </button>
          </div>
          <div className='max-w-full break-all text-12px text-t-tertiary' style={{ fontVariantNumeric: 'tabular-nums' }}>
            {qrUrl}
          </div>
        </div>
      ) : null}
    </div>
  );
}
