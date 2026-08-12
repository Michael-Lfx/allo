/**
 * Vertical Skill picker — Popover panel matching the Mode “choose a …” menu.
 * Create flow opens a centered modal hosted by the parent (not this popover).
 */
import React, { useEffect, useMemo, useState } from 'react';
import { Input, Spin } from '@arco-design/web-react';
import { CheckSmall, Plus, Upload } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import {
  importVerticalSkill,
  listVerticalSkills,
  publishVerticalSkill,
} from '../api';
import type { VerticalSkillSummary } from '../types';
import homeStyles from './home.module.css';
import styles from './verticalSkillHub.module.css';

type MenuTab = 'all' | 'user' | 'hub';

export interface VerticalSkillMenuProps {
  selectedIds: string[];
  onChangeSelected: (ids: string[]) => void;
  onRequestCreate?: () => void;
  /** Sync catalog into parent for input-area skill chips. */
  onCatalogChange?: (skills: VerticalSkillSummary[]) => void;
  /** Bump from parent after create so the list refreshes when reopened. */
  reloadToken?: number;
}

const VerticalSkillMenu: React.FC<VerticalSkillMenuProps> = ({
  selectedIds,
  onChangeSelected,
  onRequestCreate,
  onCatalogChange,
  reloadToken = 0,
}) => {
  const { t } = useTranslation();
  const [message, messageHolder] = useArcoMessage();
  const [tab, setTab] = useState<MenuTab>('all');
  const [loading, setLoading] = useState(false);
  const [skills, setSkills] = useState<VerticalSkillSummary[]>([]);
  const [query, setQuery] = useState('');
  const [reloadNonce, setReloadNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const source = tab === 'all' ? undefined : tab;
    void listVerticalSkills({ source })
      .then((list) => {
        if (cancelled) return;
        setSkills(list);
        onCatalogChange?.(list);
      })
      .catch((error) => {
        if (!cancelled) {
          message.error(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // Intentionally omit onCatalogChange — parent may pass an inline merger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [message, reloadNonce, reloadToken, tab]);

  const refresh = () => setReloadNonce((n) => n + 1);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter((skill) => {
      const hay = [skill.display_name, skill.name, skill.description, skill.category, ...skill.tags]
        .join(' ')
        .toLowerCase();
      return hay.includes(q);
    });
  }, [query, skills]);

  const toggle = (id: string) => {
    if (selectedIds.includes(id)) {
      onChangeSelected(selectedIds.filter((item) => item !== id));
    } else {
      onChangeSelected([...selectedIds, id]);
    }
  };

  const handleImport = async () => {
    if (!isDesktopShell()) {
      message.warning(
        t('videoGeneration.skills.importDesktopOnly', {
          defaultValue: '导入 Skill 仅桌面端可用。',
        })
      );
      return;
    }
    try {
      const result = await ipcBridge.dialog.showOpen.invoke({
        properties: ['openDirectory'],
      });
      const path = Array.isArray(result) ? result[0] : result;
      if (!path || typeof path !== 'string') return;
      const skill = await importVerticalSkill(path);
      message.success(
        t('videoGeneration.skills.importOk', {
          name: skill.display_name || skill.name,
          defaultValue: '已导入 {{name}}',
        })
      );
      setTab('user');
      refresh();
      if (!selectedIds.includes(skill.id)) {
        onChangeSelected([...selectedIds, skill.id]);
      }
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handlePublishLocal = async (skill: VerticalSkillSummary, event: React.MouseEvent) => {
    event.stopPropagation();
    try {
      await publishVerticalSkill(skill.id);
      message.success(
        t('videoGeneration.skills.publishOk', {
          defaultValue: '已发布到本地 Skill Hub',
        })
      );
      refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <>
      {messageHolder}
      <div
        className={`${homeStyles.slashMenu} ${styles.menuShell}`}
        role='listbox'
        aria-label={t('videoGeneration.skills.menuAria', {
          defaultValue: '选择 Skill',
        })}
      >
        <div className={homeStyles.slashMenuTitle}>
          {t('videoGeneration.skills.menuTitle', {
            defaultValue: '选择 Skill',
          })}
        </div>

        <div className={styles.tabs}>
          {(
            [
              ['all', '推荐'],
              ['hub', 'Hub'],
              ['user', '我的'],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              type='button'
              className={`${styles.tab} ${tab === key ? styles.tabActive : ''}`}
              onClick={() => setTab(key)}
            >
              {t(`videoGeneration.skills.tabs.${key}`, {
                defaultValue: label,
              })}
            </button>
          ))}
        </div>

        <Input
          allowClear
          size='small'
          className={styles.search}
          value={query}
          onChange={setQuery}
          placeholder={t('videoGeneration.skills.searchPlaceholder', {
            defaultValue: '搜索技能…',
          })}
        />

        <div className={styles.listScroll}>
          {loading ? (
            <div className={styles.loading}>
              <Spin size={18} />
            </div>
          ) : filtered.length === 0 ? (
            <div className={styles.empty}>
              {t('videoGeneration.skills.empty', { defaultValue: '暂无可用 Skill' })}
            </div>
          ) : (
            filtered.map((skill) => {
              const active = selectedIds.includes(skill.id);
              return (
                <button
                  key={skill.id}
                  type='button'
                  role='option'
                  aria-selected={active}
                  className={`${homeStyles.slashMenuItem} ${styles.skillItem} ${
                    active ? homeStyles.slashMenuItemActive : ''
                  }`}
                  onClick={() => toggle(skill.id)}
                >
                  <span className={styles.checkSlot} aria-hidden='true'>
                    {active ? <CheckSmall size={14} /> : null}
                  </span>
                  <span className={styles.skillText}>
                    <strong>{skill.display_name}</strong>
                    <small>
                      <span className={styles.skillDesc}>{skill.description}</span>
                      {skill.source === 'user' ||
                      skill.source === 'hub' ||
                      skill.source === 'builtin' ? (
                        <>
                          <span className={styles.metaDiamond} aria-hidden='true' />
                          <span className={styles.sourceBadge}>
                            {skill.source === 'user'
                              ? t('videoGeneration.skills.source.user', {
                                  defaultValue: '我的',
                                })
                              : skill.source === 'hub'
                                ? t('videoGeneration.skills.source.hub', {
                                    defaultValue: 'Hub',
                                  })
                                : t('videoGeneration.skills.source.builtin', {
                                    defaultValue: '官方',
                                  })}
                          </span>
                        </>
                      ) : null}
                    </small>
                  </span>
                  {skill.source === 'user' ? (
                    <em
                      className={styles.inlineAction}
                      onClick={(event) => void handlePublishLocal(skill, event)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          void handlePublishLocal(skill, event as never);
                        }
                      }}
                      role='button'
                      tabIndex={0}
                    >
                      {t('videoGeneration.skills.publish', { defaultValue: '发布' })}
                    </em>
                  ) : null}
                </button>
              );
            })
          )}
        </div>

        {selectedIds.length > 0 ? (
          <div className={styles.selectedHint}>
            <span>
              {t('videoGeneration.skills.selectedCount', {
                count: selectedIds.length,
                defaultValue: '已选 {{count}} 个',
              })}
            </span>
            <button type='button' onClick={() => onChangeSelected([])}>
              {t('videoGeneration.skills.clearSelected', { defaultValue: '清空' })}
            </button>
          </div>
        ) : null}

        <div className={styles.menuFooter}>
          <button
            type='button'
            className={styles.footerGhost}
            onClick={() => void handleImport()}
          >
            <Upload size={12} />
            {t('videoGeneration.skills.import', { defaultValue: '导入' })}
          </button>
          <button
            type='button'
            className={styles.footerPrimary}
            onClick={() => onRequestCreate?.()}
          >
            <Plus size={12} />
            {t('videoGeneration.skills.create', { defaultValue: '创建' })}
          </button>
        </div>
        <p className={styles.cloudHint}>
          {t('videoGeneration.skills.cloudPublishHint', {
            defaultValue: '社区发布将走云端 Skill Hub（需登录）；本地发布仅本机可见。',
          })}
        </p>
      </div>
    </>
  );
};

export default VerticalSkillMenu;
