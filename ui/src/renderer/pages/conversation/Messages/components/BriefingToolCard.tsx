import React from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';

interface BriefingToolPayload {
  briefing_id?: string;
  status?: string;
  stage?: string;
  message?: string;
  workspace?: string;
  title?: string;
}

function parseBriefingPayload(raw: unknown): BriefingToolPayload | null {
  if (raw && typeof raw === 'object' && 'briefing_id' in raw) {
    return raw as BriefingToolPayload;
  }
  if (typeof raw !== 'string') return null;
  try {
    const parsed = JSON.parse(raw) as BriefingToolPayload;
    if (parsed && typeof parsed.briefing_id === 'string') return parsed;
  } catch {
    return null;
  }
  return null;
}

export function isBriefingToolName(name: string): boolean {
  return name === 'briefing_create' || name === 'briefing_status';
}

export function BriefingToolCard({ result }: { result: unknown }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const payload = parseBriefingPayload(result);
  if (!payload?.briefing_id) return null;
  const workspace =
    payload.workspace || `/video-generation/briefing/${payload.briefing_id}`;

  return (
    <div className='rd-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-14px py-12px'>
      <div className='text-13px font-600 text-[var(--color-text-1)]'>
        {payload.title ||
          t('videoGeneration.briefing.openWorkspace', { defaultValue: '打开资讯播报工作台' })}
      </div>
      <p className='m-0 mt-4px text-12px text-[var(--color-text-3)]'>
        {payload.status}
        {payload.stage ? ` · ${payload.stage}` : ''}
        {payload.message ? ` · ${payload.message}` : ''}
      </p>
      <Button
        type='primary'
        size='small'
        className='mt-10px'
        onClick={() => navigate(workspace)}
      >
        {t('videoGeneration.briefing.openWorkspace', { defaultValue: '打开资讯播报工作台' })}
      </Button>
    </div>
  );
}
