import React from 'react';
import { useTranslation } from 'react-i18next';
import HubPageShell from '@renderer/components/layout/HubPageShell';
import LinkPairingPanel from '@renderer/pages/openCapabilities/LinkPairingPanel';
import BrowserAccessPanel from './BrowserAccessPanel';

const LinkPhonePage: React.FC = () => {
  const { t } = useTranslation();

  return (
    <HubPageShell
      title={t('settings.link.title', { defaultValue: '连接手机' })}
      subtitle={t('settings.link.subtitle', {
        defaultValue: '不安装 App 时，用微信或其他扫码工具打开网页。安装手机版时再生成配对码，并在这里允许。',
      })}
      maxWidthClass='md:max-w-720px'
    >
      <div className='flex max-w-640px flex-col gap-16px'>
        <section className='rd-12px border border-solid border-arco-2 bg-fill-0 p-16px'>
          <BrowserAccessPanel />
        </section>
        <section className='rd-12px border border-solid border-arco-2 bg-fill-0 p-16px'>
          <div className='text-16px font-600 text-t-primary'>{t('settings.link.appTitle', { defaultValue: '用 Flowy 手机版' })}</div>
          <div className='mt-4px mb-14px text-13px text-t-secondary leading-relaxed'>
            {t('settings.link.appDesc', {
              defaultValue: '安装 App 后扫描配对码。允许之前，手机拿不到登录。对话正文不经过云端。',
            })}
          </div>
          <LinkPairingPanel />
        </section>
      </div>
    </HubPageShell>
  );
};

export default LinkPhonePage;
