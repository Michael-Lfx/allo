import React from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@arco-design/web-react';
import { AttachCloseIcon } from '../home/ComposerIcons';
import type { CameoDraftItem } from '../types';
import styles from './CameoCastEditor.module.css';

export interface CameoCastEditorProps {
  value: CameoDraftItem[];
  onChange: (next: CameoDraftItem[]) => void;
  disabled?: boolean;
}

const CameoCastEditor: React.FC<CameoCastEditorProps> = ({ value, onChange, disabled }) => {
  const { t } = useTranslation();

  const removeAt = (localId: string) => {
    const target = value.find((item) => item.localId === localId);
    if (target?.previewUrl) {
      try {
        URL.revokeObjectURL(target.previewUrl);
      } catch {
        // ignore
      }
    }
    onChange(value.filter((item) => item.localId !== localId));
  };

  const patch = (localId: string, patchValue: Partial<CameoDraftItem>) => {
    onChange(value.map((item) => (item.localId === localId ? { ...item, ...patchValue } : item)));
  };

  if (value.length === 0) return null;

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <div className={styles.title}>
          {t('videoGeneration.create.cameo.title', { defaultValue: '参考图' })}
        </div>
        <p className={styles.hint}>
          {t('videoGeneration.create.cameo.hint', {
            defaultValue: '可补充标签与描述，规划时用于识别人物、场景或画风。',
          })}
        </p>
      </div>
      <div className={styles.strip}>
        {value.map((item) => (
          <div key={item.localId} className={styles.card}>
            <div className={styles.thumb}>
              {item.previewUrl ? (
                <img src={item.previewUrl} alt={item.characterName} />
              ) : null}
              <button
                type='button'
                className={styles.remove}
                disabled={disabled}
                aria-label={t('videoGeneration.create.cameo.remove', { defaultValue: '移除' })}
                onClick={() => removeAt(item.localId)}
              >
                <AttachCloseIcon />
              </button>
            </div>
            <div className={styles.fields}>
              <Input
                size='small'
                value={item.characterName}
                disabled={disabled}
                placeholder={t('videoGeneration.create.cameo.namePlaceholder', {
                  defaultValue: '标签（可选）',
                })}
                onChange={(characterName) => patch(item.localId, { characterName })}
              />
              <Input
                size='small'
                value={item.description}
                disabled={disabled}
                placeholder={t('videoGeneration.create.cameo.descPlaceholder', {
                  defaultValue: '描述（可选）',
                })}
                onChange={(description) => patch(item.localId, { description })}
              />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default CameoCastEditor;
