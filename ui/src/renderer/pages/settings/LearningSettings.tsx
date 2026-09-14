import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, InputNumber, Slider, Switch } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { useWideContentLayout } from '@/renderer/pages/learning/components/ContentWidthToggle';
import {
  SettingsControlGroup,
  SettingsList,
  SettingsPageHeader,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import PreferenceRow from '@/renderer/components/settings/SettingsModal/contents/SystemModalContent/PreferenceRow';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const DEFAULT_DESIRED_RETENTION = 0.9;
const DEFAULT_REVIEW_SESSION_LIMIT = 30;
const DEFAULT_DIAGNOSTIC_LIMIT = 10;
const DEFAULT_DAILY_CHECKIN_GOAL = 15;

const LearningSettings: React.FC = () => {
  const { t } = useTranslation();
  const [desiredRetention, setDesiredRetention] = useConfig('learning.desiredRetention');
  const [fsrsParameters, setFsrsParameters] = useConfig('learning.fsrsParameters');
  const [reviewSessionLimit, setReviewSessionLimit] = useConfig('learning.reviewSessionLimit');
  const [diagnosticLimit, setDiagnosticLimit] = useConfig('learning.diagnosticLimit');
  const [dailyCheckinGoal, setDailyCheckinGoal] = useConfig('learning.dailyCheckinGoal');
  // 内容宽度偏好与学习页头部按钮共用同一条读写路径（同一个键、同一套归一化），
  // 避免两个入口写出两种存档形态。
  const { wide: wideContent, setWide: setWideContent } = useWideContentLayout();

  const [parametersDraft, setParametersDraft] = useState('');

  useEffect(() => {
    setParametersDraft((fsrsParameters ?? []).join(', '));
  }, [fsrsParameters]);

  const saveParameters = () => {
    const trimmed = parametersDraft.trim();
    if (!trimmed) {
      void setFsrsParameters(undefined);
      Message.success(t('learning.settings.saved'));
      return;
    }
    const values = trimmed
      .split(/[,\s]+/)
      .filter(Boolean)
      .map(Number);
    if (values.some((value) => !Number.isFinite(value))) {
      Message.error(t('learning.settings.parametersInvalid'));
      return;
    }
    void setFsrsParameters(values);
    Message.success(t('learning.settings.saved'));
  };

  return (
    <SettingsPageWrapper>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={t('learning.settings.title')}
          description={t('learning.settings.description')}
        />
        <SettingsList>
            <PreferenceRow
              label={t('learning.settings.wideContent')}
              description={t('learning.settings.wideContentHint')}
              controlLayout='field'
            >
              <Switch checked={wideContent} onChange={(value) => void setWideContent(value)} />
            </PreferenceRow>
            <PreferenceRow
              label={t('learning.settings.desiredRetention')}
              description={t('learning.settings.desiredRetentionHint')}
              controlLayout='compound'
            >
              <div className='flex items-center gap-12px w-full sm:w-280px'>
                <Slider
                  className='flex-1'
                  min={0.7}
                  max={0.99}
                  step={0.01}
                  value={desiredRetention ?? DEFAULT_DESIRED_RETENTION}
                  onChange={(value) => void setDesiredRetention(Number(value))}
                />
                <InputNumber
                  className='w-90px shrink-0'
                  min={0.7}
                  max={0.99}
                  step={0.01}
                  value={desiredRetention ?? DEFAULT_DESIRED_RETENTION}
                  onChange={(value) => value != null && void setDesiredRetention(Number(value))}
                />
              </div>
            </PreferenceRow>
            <PreferenceRow
              label={t('learning.settings.fsrsParameters')}
              description={t('learning.settings.fsrsParametersHint')}
              controlLayout='compound'
            >
              <div className='flex flex-col gap-8px w-full'>
                <Input.TextArea
                  autoSize={{ minRows: 2, maxRows: 4 }}
                  placeholder={(fsrsParameters ?? []).join(', ')}
                  value={parametersDraft}
                  onChange={setParametersDraft}
                />
                <SettingsControlGroup>
                  <Button size='small' onClick={saveParameters}>
                    {t('learning.settings.fsrsParametersApply')}
                  </Button>
                </SettingsControlGroup>
              </div>
            </PreferenceRow>
            <PreferenceRow
              label={t('learning.settings.reviewSessionLimit')}
              description={t('learning.settings.reviewSessionLimitHint')}
              controlLayout='field'
            >
              <InputNumber
                className='w-full sm:w-180px'
                min={1}
                max={100}
                value={reviewSessionLimit ?? DEFAULT_REVIEW_SESSION_LIMIT}
                onChange={(value) => value != null && void setReviewSessionLimit(Number(value))}
              />
            </PreferenceRow>
            <PreferenceRow
              label={t('learning.settings.dailyCheckinGoal')}
              description={t('learning.settings.dailyCheckinGoalHint')}
              controlLayout='field'
            >
              <InputNumber
                className='w-full sm:w-180px'
                min={0}
                max={500}
                value={dailyCheckinGoal ?? DEFAULT_DAILY_CHECKIN_GOAL}
                onChange={(value) => value != null && void setDailyCheckinGoal(Number(value))}
              />
            </PreferenceRow>
            <PreferenceRow
              label={t('learning.settings.diagnosticLimit')}
              description={t('learning.settings.diagnosticLimitHint')}
              controlLayout='field'
            >
              <InputNumber
                className='w-full sm:w-180px'
                min={1}
                max={50}
                value={diagnosticLimit ?? DEFAULT_DIAGNOSTIC_LIMIT}
                onChange={(value) => value != null && void setDiagnosticLimit(Number(value))}
              />
            </PreferenceRow>
        </SettingsList>
      </div>
    </SettingsPageWrapper>
  );
};

export default LearningSettings;
