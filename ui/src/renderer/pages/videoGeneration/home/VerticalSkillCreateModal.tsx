/**
 * Create Skill modal — field layout aligned with LibTV
 * https://www.liblib.tv/skill/create
 */
import React, { useMemo, useRef, useState } from 'react';
import { Input, Modal, Select } from '@arco-design/web-react';
import { CloseSmall, Upload } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { createVerticalSkill, publishVerticalSkillToCloud } from '../api';
import type { VerticalSkillDraft } from '../types';
import styles from './verticalSkillHub.module.css';

/** Cloud Skill Hub categories — must match server enum. */
const SKILL_HUB_CATEGORIES = [
  { value: 'short-drama', labelKey: 'shortDrama', defaultLabel: '短漫剧' },
  { value: 'film', labelKey: 'film', defaultLabel: '电影' },
  { value: 'advertising', labelKey: 'advertising', defaultLabel: '商业广告' },
  { value: 'creative-social', labelKey: 'creativeSocial', defaultLabel: '创意/社媒玩法' },
  { value: 'music-mv', labelKey: 'musicMv', defaultLabel: '音乐 MV' },
] as const;

const PLAYBOOK_TEMPLATE = `## 做什么
（一句话说明用途）例：把一句话故事想法做成一条短漫剧成片

## 需要什么输入
（最少提供什么）例：一句话想法，可选画风、时长、主角设定

## 怎么做
（写你在意的环节和要求，不用写全）例：脚本要反转多，画风固定成韩漫

## 产出什么
（最终交付什么）例：成片，附脚本和分镜

## 什么时候问你
（什么情况下停下来问你）例：拿不准题材或风格时问一次，其余自己定
`;

const COVER_ACCEPT = 'image/png,image/jpeg,image/jpg,image/webp,image/gif';
const COVER_MAX_BYTES = 5 * 1024 * 1024;

type CreateFormState = {
  display_name: string;
  description: string;
  category: string;
  use_scenario: string;
  how_to_use: string;
  output: string;
  playbook: string;
  /** Persisted cover (data URL or remote URL). */
  cover_url: string;
};

const EMPTY_FORM: CreateFormState = {
  display_name: '',
  description: '',
  category: 'creative-social',
  use_scenario: '',
  how_to_use: '',
  output: '',
  playbook: '',
  cover_url: '',
};

function toKebabName(raw: string): string {
  return raw
    .trim()
    .toLowerCase()
    .replace(/[_\s]+/g, '-')
    .replace(/[^a-z0-9-]/g, '')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 64);
}

function resolveSkillName(displayName: string): string {
  const fromDisplay = toKebabName(displayName);
  if (fromDisplay && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(fromDisplay)) {
    return fromDisplay;
  }
  return `skill-${Date.now().toString(36)}`;
}

function composePlaybook(form: CreateFormState): string {
  const body = form.playbook.trim() || PLAYBOOK_TEMPLATE.trim();
  const extras: string[] = [];
  if (form.use_scenario.trim()) {
    extras.push(`## 使用场景\n${form.use_scenario.trim()}`);
  }
  if (form.how_to_use.trim()) {
    extras.push(`## 如何使用\n${form.how_to_use.trim()}`);
  }
  if (form.output.trim()) {
    extras.push(`## 输出内容\n${form.output.trim()}`);
  }
  if (extras.length === 0) return body;
  const lower = body.toLowerCase();
  const filtered = extras.filter((block) => {
    const heading = block.split('\n')[0].replace(/^##\s*/, '').toLowerCase();
    return !lower.includes(heading);
  });
  if (filtered.length === 0) return body;
  return `${body}\n\n${filtered.join('\n\n')}`;
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === 'string') resolve(reader.result);
      else reject(new Error('failed to read image'));
    };
    reader.onerror = () => reject(reader.error ?? new Error('failed to read image'));
    reader.readAsDataURL(file);
  });
}

export interface VerticalSkillCreateModalProps {
  visible: boolean;
  onClose: () => void;
  onCreated: (skillId: string) => void;
}

const VerticalSkillCreateModal: React.FC<VerticalSkillCreateModalProps> = ({
  visible,
  onClose,
  onCreated,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status: cloudStatus } = useCloudAuth();
  const [message, messageHolder] = useArcoMessage();
  const [form, setForm] = useState<CreateFormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const mdInputRef = useRef<HTMLInputElement>(null);
  const coverInputRef = useRef<HTMLInputElement>(null);

  const inferredName = useMemo(
    () => resolveSkillName(form.display_name),
    [form.display_name]
  );

  const reset = () => {
    setForm(EMPTY_FORM);
  };

  const handleClose = () => {
    if (saving) return;
    reset();
    onClose();
  };

  const applyMdFile = async (file: File) => {
    const text = await file.text();
    const trimmed = text.trim();
    if (!trimmed) {
      message.warning(
        t('videoGeneration.skills.mdEmpty', { defaultValue: 'MD 文件内容为空。' })
      );
      return;
    }
    const body = trimmed.startsWith('---')
      ? trimmed.replace(/^---[\s\S]*?---\s*/, '').trim()
      : trimmed;
    if (form.playbook.trim()) {
      Modal.confirm({
        title: t('videoGeneration.skills.mdOverwriteTitle', {
          defaultValue: '覆盖现有内容',
        }),
        content: t('videoGeneration.skills.mdOverwriteDesc', {
          defaultValue: '当前 Skill 内容已有文本，确认用上传的 .md 文件覆盖吗？',
        }),
        okText: t('videoGeneration.skills.mdOverwrite', { defaultValue: '覆盖' }),
        onOk: () => setForm((current) => ({ ...current, playbook: body })),
      });
    } else {
      setForm((current) => ({ ...current, playbook: body }));
    }
  };

  const applyCoverFile = async (file: File) => {
    if (!file.type.startsWith('image/')) {
      message.warning(
        t('videoGeneration.skills.coverTypeInvalid', {
          defaultValue: '请上传图片文件（PNG / JPEG / WEBP / GIF）。',
        })
      );
      return;
    }
    if (file.size > COVER_MAX_BYTES) {
      message.warning(
        t('videoGeneration.skills.coverTooLarge', {
          defaultValue: '封面图不能超过 5MB。',
        })
      );
      return;
    }
    try {
      const dataUrl = await readFileAsDataUrl(file);
      setForm((current) => ({ ...current, cover_url: dataUrl }));
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleCreate = async (mode: 'local' | 'cloud') => {
    const displayName = form.display_name.trim();
    if (!displayName) {
      message.warning(
        t('videoGeneration.skills.createSkillErrName', {
          defaultValue: '请输入 Skill 名称',
        })
      );
      return;
    }
    if (!form.description.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrDescription', {
          defaultValue: '请输入一句话介绍',
        })
      );
      return;
    }
    if (!form.category.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrCategory', {
          defaultValue: '请选择类型',
        })
      );
      return;
    }
    if (!form.playbook.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrContent', {
          defaultValue: '请填写 Skill 内容',
        })
      );
      return;
    }
    if (!form.use_scenario.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrUseScenario', {
          defaultValue: '请输入使用场景',
        })
      );
      return;
    }
    if (!form.how_to_use.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrHowToUse', {
          defaultValue: '请输入如何使用',
        })
      );
      return;
    }
    if (!form.output.trim()) {
      message.warning(
        t('videoGeneration.skills.createSkillErrOutput', {
          defaultValue: '请输入输出内容',
        })
      );
      return;
    }

    const coverRaw = form.cover_url.trim();
    // Avoid bloating SKILL.md with base64; pass data URLs only to cloud publish.
    const coverForDraft =
      coverRaw.startsWith('http://') || coverRaw.startsWith('https://')
        ? coverRaw
        : undefined;

    const draft: VerticalSkillDraft = {
      name: inferredName,
      display_name: displayName,
      description: form.description.trim(),
      category: form.category.trim(),
      version: '1.0.0',
      tags: [],
      compatible_modes: [],
      use_scenario: form.use_scenario.trim(),
      how_to_use: form.how_to_use.trim(),
      output: form.output.trim(),
      cover_url: coverForDraft,
      playbook: composePlaybook(form),
    };

    setSaving(true);
    try {
      const created = await createVerticalSkill(draft);
      if (mode === 'cloud') {
        const result = await publishVerticalSkillToCloud(created.id, {
          coverUrl: coverRaw || undefined,
        });
        message.success(
          t('videoGeneration.skills.createAndPublishOk', {
            name: created.display_name || created.name,
            status: result.status,
            defaultValue: '已创建并提交社区审核（{{status}}）',
          })
        );
      } else {
        message.success(
          t('videoGeneration.skills.createOk', {
            name: created.display_name || created.name,
            defaultValue: '已创建 {{name}}',
          })
        );
      }
      reset();
      onCreated(created.id);
      onClose();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const handleCreateLocal = () => {
    void handleCreate('local');
  };

  const handleCreateAndPublish = () => {
    if (cloudStatus !== 'authenticated') {
      message.warning(
        t('videoGeneration.skills.authRequired.publish', {
          defaultValue: '发布到社区需要先登录云端账号',
        })
      );
      navigate('/cloud-login');
      return;
    }
    void handleCreate('cloud');
  };

  return (
    <>
      {messageHolder}
      <Modal
        title={t('videoGeneration.skills.createTitle', {
          defaultValue: '创建 Skill',
        })}
        visible={visible}
        onCancel={handleClose}
        confirmLoading={saving}
        style={{ width: 640 }}
        unmountOnExit
        maskClosable={!saving}
        wrapClassName={styles.createModalWrap}
        className={styles.createModal}
        alignCenter
        footer={
          <div className={styles.createFooter}>
            <button
              type='button'
              className={styles.createFooterCancel}
              disabled={saving}
              onClick={handleClose}
            >
              {t('videoGeneration.skills.backToList', { defaultValue: '取消' })}
            </button>
            <div className={styles.createFooterActions}>
              <button
                type='button'
                className={styles.createFooterSecondary}
                disabled={saving}
                onClick={handleCreateLocal}
              >
                {saving
                  ? t('videoGeneration.skills.createSaving', { defaultValue: '创建中…' })
                  : t('videoGeneration.skills.createSubmit', {
                      defaultValue: '创建并选用',
                    })}
              </button>
              <button
                type='button'
                className={styles.createFooterSubmit}
                disabled={saving}
                onClick={handleCreateAndPublish}
              >
                {saving
                  ? t('videoGeneration.skills.createPublishing', {
                      defaultValue: '发布中…',
                    })
                  : t('videoGeneration.skills.createAndPublish', {
                      defaultValue: '创建并发布到社区',
                    })}
              </button>
            </div>
          </div>
        }
      >
        <div className={styles.createForm}>
          <p className={styles.createLead}>
            {t('videoGeneration.skills.createLead', {
              defaultValue:
                '一个 Skill，一部作品。写清名称、一句话介绍与 Skill 内容，并补充使用场景 / 如何使用 / 输出。',
            })}
          </p>

          <section className={styles.createSection}>
            <header className={styles.createSectionTitle}>
              {t('videoGeneration.skills.sections.basics', { defaultValue: '基本信息' })}
            </header>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.skillName', {
                  defaultValue: 'Skill 名称',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Input
                value={form.display_name}
                maxLength={64}
                placeholder={t('videoGeneration.skills.placeholders.skillName', {
                  defaultValue: '给你的 Skill 起个名字',
                })}
                onChange={(value) =>
                  setForm((current) => ({ ...current, display_name: value }))
                }
              />
            </label>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.oneLiner', {
                  defaultValue: '一句话介绍',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Input.TextArea
                value={form.description}
                maxLength={500}
                showWordLimit
                autoSize={{ minRows: 2, maxRows: 3 }}
                placeholder={t('videoGeneration.skills.placeholders.oneLiner', {
                  defaultValue: '简短描述该 Skill 的能力',
                })}
                onChange={(value) =>
                  setForm((current) => ({ ...current, description: value }))
                }
              />
            </label>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.resultType', {
                  defaultValue: '选择类型',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Select
                value={form.category}
                placeholder={t('videoGeneration.skills.placeholders.category', {
                  defaultValue: '请选择类型',
                })}
                onChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    category: typeof value === 'string' ? value : 'creative-social',
                  }))
                }
              >
                {SKILL_HUB_CATEGORIES.map((item) => (
                  <Select.Option key={item.value} value={item.value}>
                    {t(`videoGeneration.skills.categories.${item.labelKey}`, {
                      defaultValue: item.defaultLabel,
                    })}
                  </Select.Option>
                ))}
              </Select>
            </label>

            <div className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.coverOptional', {
                  defaultValue: '上传封面（选填）',
                })}
              </span>
              <p className={styles.createSectionHint}>
                {t('videoGeneration.skills.placeholders.coverHint', {
                  defaultValue: '建议横版图片，支持 PNG / JPEG / WEBP / GIF，最大 5MB。',
                })}
              </p>
              {form.cover_url ? (
                <div className={styles.coverPreview}>
                  <img src={form.cover_url} alt='' />
                  <button
                    type='button'
                    className={styles.coverRemove}
                    aria-label={t('videoGeneration.skills.removeCover', {
                      defaultValue: '移除封面',
                    })}
                    onClick={() =>
                      setForm((current) => ({ ...current, cover_url: '' }))
                    }
                  >
                    <CloseSmall size={12} />
                  </button>
                  <button
                    type='button'
                    className={styles.coverReplace}
                    onClick={() => coverInputRef.current?.click()}
                  >
                    {t('videoGeneration.skills.replaceCover', {
                      defaultValue: '更换',
                    })}
                  </button>
                </div>
              ) : (
                <button
                  type='button'
                  className={styles.coverUpload}
                  onClick={() => coverInputRef.current?.click()}
                >
                  <Upload size={18} />
                  <span>
                    {t('videoGeneration.skills.uploadCover', {
                      defaultValue: '上传封面图片',
                    })}
                  </span>
                </button>
              )}
              <input
                ref={coverInputRef}
                type='file'
                accept={COVER_ACCEPT}
                hidden
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  if (file) void applyCoverFile(file);
                }}
              />
            </div>
          </section>

          <section className={styles.createSection}>
            <header className={styles.createSectionTitle}>
              {t('videoGeneration.skills.fields.content', {
                defaultValue: 'Skill 内容',
              })}
              <em className={styles.fieldRequired}>*</em>
            </header>
            <p className={styles.createSectionHint}>
              {t('videoGeneration.skills.sections.contentHint', {
                defaultValue:
                  '按「做什么 / 需要什么输入 / 怎么做 / 产出什么 / 什么时候问你」书写；也可直接上传 SKILL.md。',
              })}
            </p>
            <div className={styles.contentToolbar}>
              <button
                type='button'
                className={styles.footerGhost}
                onClick={() =>
                  setForm((current) => ({
                    ...current,
                    playbook: current.playbook.trim()
                      ? current.playbook
                      : PLAYBOOK_TEMPLATE,
                  }))
                }
              >
                {t('videoGeneration.skills.insertTemplate', {
                  defaultValue: '插入模板',
                })}
              </button>
              <button
                type='button'
                className={styles.footerGhost}
                onClick={() => mdInputRef.current?.click()}
              >
                <Upload size={12} />
                {t('videoGeneration.skills.uploadMd', {
                  defaultValue: '上传 MD 文件',
                })}
              </button>
              <input
                ref={mdInputRef}
                type='file'
                accept='.md,text/markdown,text/plain'
                hidden
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  if (file) void applyMdFile(file);
                }}
              />
            </div>
            <Input.TextArea
              value={form.playbook}
              autoSize={{ minRows: 8, maxRows: 14 }}
              className={styles.playbookInput}
              placeholder={t('videoGeneration.skills.placeholders.content', {
                defaultValue: `输入 Skill 内容，或上传 SKILL.md 文件直接替换\n\n${PLAYBOOK_TEMPLATE}`,
              })}
              onChange={(value) =>
                setForm((current) => ({ ...current, playbook: value }))
              }
            />
          </section>

          <section className={styles.createSection}>
            <header className={styles.createSectionTitle}>
              {t('videoGeneration.skills.sections.usage', {
                defaultValue: '使用说明',
              })}
            </header>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.useScenario', {
                  defaultValue: '使用场景',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Input.TextArea
                value={form.use_scenario}
                autoSize={{ minRows: 2, maxRows: 4 }}
                placeholder={t('videoGeneration.skills.placeholders.useScenario', {
                  defaultValue: '详细描述该 Skill 的使用场景信息',
                })}
                onChange={(value) =>
                  setForm((current) => ({ ...current, use_scenario: value }))
                }
              />
            </label>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.howToUse', {
                  defaultValue: '如何使用',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Input.TextArea
                value={form.how_to_use}
                autoSize={{ minRows: 2, maxRows: 4 }}
                placeholder={t('videoGeneration.skills.placeholders.howToUse', {
                  defaultValue:
                    '描述用户如何使用该 Skill，需要输入什么信息（例如：剧本内容、故事梗概或任何叙事素材）',
                })}
                onChange={(value) =>
                  setForm((current) => ({ ...current, how_to_use: value }))
                }
              />
            </label>

            <label className={styles.field}>
              <span>
                {t('videoGeneration.skills.fields.output', {
                  defaultValue: '输出内容',
                })}
                <em className={styles.fieldRequired}>*</em>
              </span>
              <Input.TextArea
                value={form.output}
                autoSize={{ minRows: 2, maxRows: 4 }}
                placeholder={t('videoGeneration.skills.placeholders.output', {
                  defaultValue:
                    '描述用户使用该 Skill 后，预期输出的结果产物是什么（例如：90秒超现实主义数字片头视频）',
                })}
                onChange={(value) =>
                  setForm((current) => ({ ...current, output: value }))
                }
              />
            </label>
          </section>
        </div>
      </Modal>
    </>
  );
};

export default VerticalSkillCreateModal;
