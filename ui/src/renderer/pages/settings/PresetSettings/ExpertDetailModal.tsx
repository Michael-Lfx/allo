import { Modal } from '@arco-design/web-react';
import { Light, Lightning, Message } from '@icon-park/react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import type { IExpertDetail, IExpertSummary } from '@/common/adapter/ipcBridge';
import SettingsContentLoading from '@/renderer/components/layout/SettingsContentLoading';
import {
  expertDetailErrorKind,
  localizedPair,
  localizedTags,
  readPromptText,
  type ExpertDetailErrorKind,
} from './expertCatalogModel';
import { ExpertMark } from './ExpertTile';

type ExpertDetailModalProps = {
  expert: IExpertSummary | null;
  pending?: boolean;
  installed?: boolean;
  onClose: () => void;
  onSummon?: () => void;
};

const Divider: React.FC = () => <div className='h-1px bg-[var(--color-border-2)]' aria-hidden='true' />;

const ExpertDetailModal: React.FC<ExpertDetailModalProps> = ({
  expert,
  pending = false,
  installed = false,
  onClose,
  onSummon,
}) => {
  const { t, i18n } = useTranslation();
  const [detail, setDetail] = useState<IExpertDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [errorKind, setErrorKind] = useState<ExpertDetailErrorKind | null>(null);
  const slug = expert?.slug ?? '';

  useEffect(() => {
    if (!slug) return;
    let cancelled = false;
    setDetail(null);
    setErrorKind(null);
    setLoading(true);
    void ipcBridge.cloud.getExpert
      .invoke({ slug })
      .then((next) => {
        if (!cancelled) setDetail(next);
      })
      .catch((error: unknown) => {
        if (!cancelled) setErrorKind(expertDetailErrorKind(error));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [slug]);

  const language = i18n.language;
  const source = detail ?? expert;
  const name = localizedPair(language, source?.name, source?.nameEn) || slug;
  const profession = localizedPair(language, source?.profession, source?.professionEn);
  const summary = localizedPair(language, source?.summary, source?.summaryEn);
  const tags = localizedTags(language, source?.tags);
  const initPrompt = readPromptText(detail?.defaultInitPrompt, language);
  const quickPrompts = (detail?.quickPrompts ?? [])
    .map((prompt) => readPromptText(prompt, language))
    .filter((prompt) => prompt.length > 0);
  const helpPrompts = [...new Set([initPrompt, ...quickPrompts])].filter((prompt) => prompt.length > 0);
  const downloads = source?.downloadCount ?? 0;
  const errorText =
    errorKind === 'notFound'
      ? t('settings.experts.notFound', { defaultValue: 'This expert is not listed or does not exist.' })
      : errorKind === 'login'
        ? t('settings.experts.loginRequired', { defaultValue: 'Sign in to the cloud account to browse experts.' })
        : errorKind
          ? t('common.failed', { defaultValue: 'Failed' })
          : '';

  return (
    <Modal
      visible={Boolean(expert)}
      unmountOnExit
      onCancel={onClose}
      footer={null}
      autoFocus={false}
      style={{ width: 560, maxWidth: '92vw', borderRadius: 16 }}
      data-testid='expert-detail-modal'
    >
      {expert ? (
        <div className='flex flex-col gap-20px text-13px leading-20px text-t-primary'>
          <div className='flex items-start gap-16px pr-32px'>
            <ExpertMark name={name} avatarUrl={source?.avatarUrl} size={64} />
            <div className='flex min-w-0 flex-1 flex-col items-start gap-8px'>
              <div className='flex min-w-0 max-w-full items-baseline gap-8px'>
                <h2 className='m-0 min-w-0 truncate text-18px font-semibold leading-26px'>{name}</h2>
                {profession ? (
                  <span className='shrink-0 border-l border-l-solid border-[var(--color-border-3)] pl-8px text-14px leading-22px text-t-secondary'>
                    {profession}
                  </span>
                ) : null}
              </div>
              <button
                type='button'
                data-testid='expert-detail-summon'
                disabled={pending || installed || !onSummon}
                aria-busy={pending}
                className='cursor-pointer rounded-full border-none bg-[var(--color-text-1)] px-14px py-4px text-13px font-semibold leading-20px text-[var(--color-bg-1)] disabled:cursor-default disabled:opacity-60'
                onClick={onSummon}
              >
                {installed
                  ? t('settings.experts.summoned', { defaultValue: 'Summoned' })
                  : pending
                    ? t('settings.experts.summoning', { defaultValue: 'Summoning' })
                    : t('settings.experts.summon', { defaultValue: 'Summon' })}
              </button>
            </div>
          </div>
          {summary ? <p className='m-0'>{summary}</p> : null}
          {tags.length > 0 ? (
            <div className='flex flex-wrap gap-6px'>
              {tags.map((tag) => (
                <span key={tag} className='rounded-6px bg-fill-2 px-10px py-4px text-12px leading-18px text-t-secondary'>
                  {tag}
                </span>
              ))}
            </div>
          ) : null}
          <Divider />
          <div className='flex justify-around text-center'>
            {source?.version ? (
              <div className='flex flex-col items-center gap-4px'>
                <span className='text-15px font-semibold leading-22px'>{source.version}</span>
                <span className='text-12px leading-18px text-t-tertiary'>
                  {t('settings.experts.versionLabel', { defaultValue: 'Version' })}
                </span>
              </div>
            ) : null}
            <div className='flex flex-col items-center gap-4px'>
              <span className='flex items-center gap-4px text-15px font-semibold leading-22px'>
                <Lightning theme='outline' size={14} className='relative top-px' />
                {downloads.toLocaleString(language)}
              </span>
              <span className='text-12px leading-18px text-t-tertiary'>
                {t('settings.experts.downloads', { defaultValue: 'Downloads / uses' })}
              </span>
            </div>
          </div>
          <Divider />
          {loading ? <SettingsContentLoading className='min-h-180px' /> : null}
          {errorText ? (
            <div className='text-t-secondary' role='alert' data-testid='expert-detail-error'>
              {errorText}
            </div>
          ) : null}
          {helpPrompts.length > 0 ? (
            <section className='flex flex-col gap-10px'>
              <h3 className='m-0 flex items-center gap-6px text-14px font-semibold leading-22px'>
                <Light theme='outline' size={16} />
                {t('settings.experts.helpsWith', { defaultValue: 'What this expert can do' })}
              </h3>
              <ul className='m-0 flex list-none flex-col gap-8px p-0'>
                {helpPrompts.map((prompt) => (
                  <li
                    key={prompt}
                    className='flex items-start justify-between gap-12px rounded-12px bg-fill-2 px-14px py-12px text-t-primary'
                  >
                    <span className='min-w-0 whitespace-pre-wrap'>{prompt}</span>
                    <Message theme='outline' size={16} className='mt-2px shrink-0 text-t-secondary' />
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
        </div>
      ) : null}
    </Modal>
  );
};

export default ExpertDetailModal;
