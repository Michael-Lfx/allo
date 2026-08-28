/**
 * Vertical Skill picker — Popover panel matching the Mode “choose a …” menu.
 * Create flow opens a centered modal hosted by the parent (not this popover).
 *
 * Tabs:
 * - 推荐: official builtins only
 * - Hub: community plaza (others’ published skills; own listings stay in 我的)
 * - 我的: local user skills + cloud publish status
 */
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Input, Spin } from '@arco-design/web-react';
import { CheckSmall, Plus, Upload } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import {
  importVerticalSkill,
  installCloudSkill,
  listCloudSkills,
  listMyCloudSkills,
  listVerticalSkills,
  publishVerticalSkillToCloud,
} from '../api';
import type {
  VimaxCloudSkill,
  VimaxCloudSkillStatus,
  VerticalSkillSummary,
} from '../types';
import homeStyles from './home.module.css';
import styles from './verticalSkillHub.module.css';

type MenuTab = 'all' | 'user' | 'hub';

type LocalSkillRow = VerticalSkillSummary & {
  cloud?: VimaxCloudSkill | null;
};

export interface VerticalSkillMenuProps {
  selectedIds: string[];
  onChangeSelected: (ids: string[]) => void;
  onRequestCreate?: () => void;
  /** Sync catalog into parent for input-area skill chips. */
  onCatalogChange?: (skills: VerticalSkillSummary[]) => void;
  /** Bump from parent after create so the list refreshes when reopened. */
  reloadToken?: number;
  /** Catalog already fetched for chips; skip the empty-state spinner on first paint. */
  initialSkills?: VerticalSkillSummary[];
}

function cloudStatusLabel(
  status: VimaxCloudSkillStatus | string | undefined,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  const key = (status || '').toLowerCase();
  switch (key) {
    case 'pending':
      return t('videoGeneration.skills.cloudStatus.pending', { defaultValue: '审核中' });
    case 'published':
      return t('videoGeneration.skills.cloudStatus.published', { defaultValue: '已上架' });
    case 'offline':
      return t('videoGeneration.skills.cloudStatus.offline', { defaultValue: '已下架' });
    case 'deleted':
      return t('videoGeneration.skills.cloudStatus.deleted', { defaultValue: '已删除' });
    default:
      return t('videoGeneration.skills.cloudStatus.local', { defaultValue: '仅本地' });
  }
}

function cloudStatusClass(status: VimaxCloudSkillStatus | string | undefined): string {
  const key = (status || '').toLowerCase();
  if (key === 'pending') return styles.statusPending;
  if (key === 'published') return styles.statusPublished;
  if (key === 'offline' || key === 'deleted') return styles.statusOffline;
  return styles.statusLocal;
}

const VerticalSkillMenu: React.FC<VerticalSkillMenuProps> = ({
  selectedIds,
  onChangeSelected,
  onRequestCreate,
  onCatalogChange,
  reloadToken = 0,
  initialSkills,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status: cloudStatus, logout } = useCloudAuth();
  const [message, messageHolder] = useArcoMessage();
  const [tab, setTab] = useState<MenuTab>('all');
  const [loading, setLoading] = useState(false);
  const [skills, setSkills] = useState<VerticalSkillSummary[]>(() => initialSkills ?? []);
  const skillsRef = useRef(skills);
  skillsRef.current = skills;
  const [cloudPlaza, setCloudPlaza] = useState<VimaxCloudSkill[]>([]);
  const [myCloudByName, setMyCloudByName] = useState<Map<string, VimaxCloudSkill>>(new Map());
  const [query, setQuery] = useState('');
  const [reloadNonce, setReloadNonce] = useState(0);
  const [busyId, setBusyId] = useState<string | null>(null);

  const cloudReady = cloudStatus === 'authenticated';

  const handleCloudAuthError = async (error: unknown): Promise<boolean> => {
    if (!isInvalidCloudSessionError(error)) return false;
    await logout();
    navigate('/cloud-login');
    return true;
  };

  useEffect(() => {
    let cancelled = false;
    const showSpinner = skillsRef.current.length === 0;
    if (showSpinner) setLoading(true);

    const load = async () => {
      try {
        const localPromise = listVerticalSkills();
        const minePromise = cloudReady
          ? listMyCloudSkills({ page: 1, pageSize: 50 }).catch(() => ({
              list: [] as VimaxCloudSkill[],
            }))
          : Promise.resolve({ list: [] as VimaxCloudSkill[] });
        const plazaPromise =
          tab === 'hub' && cloudReady
            ? listCloudSkills({
                page: 1,
                pageSize: 50,
                keyword: query.trim() || undefined,
                sort: 'new',
              })
            : Promise.resolve({ list: [] as VimaxCloudSkill[] });

        const [localAll, mine, plaza] = await Promise.all([
          localPromise,
          minePromise,
          plazaPromise,
        ]);
        if (cancelled) return;

        // Prefer user: over hub: for the same name (avoid duplicate after local hub copy).
        const preferred = new Map<string, VerticalSkillSummary>();
        for (const skill of localAll) {
          const prev = preferred.get(skill.name);
          if (!prev) {
            preferred.set(skill.name, skill);
            continue;
          }
          const rank = (s: VerticalSkillSummary) =>
            s.source === 'user' ? 0 : s.source === 'hub' ? 1 : 2;
          if (rank(skill) < rank(prev)) preferred.set(skill.name, skill);
        }
        const deduped = Array.from(preferred.values());
        setSkills(deduped);
        onCatalogChange?.(deduped);

        const mineMap = new Map<string, VimaxCloudSkill>();
        for (const item of mine.list ?? []) {
          if (!item?.name) continue;
          mineMap.set(item.name, item);
          if (item.clientSkillId?.includes(':')) {
            const bare = item.clientSkillId.split(':').slice(1).join(':');
            if (bare) mineMap.set(bare, item);
          }
        }
        setMyCloudByName(mineMap);

        if (tab === 'hub') {
          setCloudPlaza(cloudReady ? plaza.list ?? [] : []);
        } else {
          setCloudPlaza([]);
        }
      } catch (error) {
        if (cancelled) return;
        if (await handleCloudAuthError(error)) return;
        message.error(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
    // Intentionally omit onCatalogChange — parent may pass an inline merger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cloudReady, message, reloadNonce, reloadToken, tab, tab === 'hub' ? query : '']);

  const refresh = () => setReloadNonce((n) => n + 1);

  const localRows: LocalSkillRow[] = useMemo(() => {
    return skills.map((skill) => ({
      ...skill,
      cloud: myCloudByName.get(skill.name) ?? null,
    }));
  }, [myCloudByName, skills]);

  const filteredRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    let rows = localRows;
    if (tab === 'all') {
      // Featured = official builtins only (my creations stay in 我的).
      rows = rows.filter((skill) => skill.source === 'builtin');
    } else if (tab === 'user') {
      rows = rows.filter((skill) => skill.source === 'user');
    }
    if (!q || tab === 'hub') return rows;
    return rows.filter((skill) => {
      const hay = [
        skill.display_name,
        skill.name,
        skill.description,
        skill.category,
        ...skill.tags,
        skill.cloud?.status ?? '',
      ]
        .join(' ')
        .toLowerCase();
      return hay.includes(q);
    });
  }, [localRows, query, tab]);

  const installedByName = useMemo(() => {
    const map = new Map<string, VerticalSkillSummary>();
    for (const skill of skills) {
      if (skill.source === 'user') map.set(skill.name, skill);
    }
    return map;
  }, [skills]);

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

  const handlePublishCloud = async (skill: VerticalSkillSummary, event: React.MouseEvent) => {
    event.stopPropagation();
    if (!cloudReady) {
      message.warning(
        t('videoGeneration.skills.authRequired.publish', {
          defaultValue: '发布到社区需要先登录云端账号',
        })
      );
      navigate('/cloud-login');
      return;
    }
    setBusyId(`cloud:${skill.id}`);
    try {
      const result = await publishVerticalSkillToCloud(skill.id);
      message.success(
        t('videoGeneration.skills.cloudPublishOk', {
          status: result.status,
          defaultValue: '已提交社区审核（{{status}}）',
        })
      );
      setTab('user');
      refresh();
    } catch (error) {
      if (await handleCloudAuthError(error)) return;
      message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyId(null);
    }
  };

  const handleInstallCloud = async (cloud: VimaxCloudSkill, event: React.MouseEvent) => {
    event.stopPropagation();
    if (!cloudReady) {
      navigate('/cloud-login');
      return;
    }
    if (cloud.isMine || myCloudByName.has(cloud.name)) {
      const mine = installedByName.get(cloud.name);
      if (mine) {
        if (!selectedIds.includes(mine.id)) {
          onChangeSelected([...selectedIds, mine.id]);
        }
        message.info(
          t('videoGeneration.skills.useOwnFromHub', {
            defaultValue: '已选用你本地的同名 Skill',
          })
        );
        return;
      }
      // Own listing without local copy: still allow install to sync package.
    }
    const existing = installedByName.get(cloud.name);
    if (existing) {
      if (!selectedIds.includes(existing.id)) {
        onChangeSelected([...selectedIds, existing.id]);
      }
      message.info(
        t('videoGeneration.skills.alreadyInstalled', {
          defaultValue: '已安装，已加入选用',
        })
      );
      return;
    }
    setBusyId(`install:${cloud.id}`);
    try {
      const skill = await installCloudSkill(cloud.id);
      message.success(
        t('videoGeneration.skills.installOk', {
          name: skill.display_name || skill.name,
          defaultValue: '已安装 {{name}}',
        })
      );
      refresh();
      if (!selectedIds.includes(skill.id)) {
        onChangeSelected([...selectedIds, skill.id]);
      }
    } catch (error) {
      if (await handleCloudAuthError(error)) return;
      message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyId(null);
    }
  };

  const publishActionLabel = (cloud?: VimaxCloudSkill | null) => {
    const status = (cloud?.status || '').toLowerCase();
    if (status === 'published') {
      return t('videoGeneration.skills.republishCloud', { defaultValue: '更新发布' });
    }
    if (status === 'offline') {
      return t('videoGeneration.skills.relistCloud', { defaultValue: '重新上架' });
    }
    if (status === 'pending') {
      return t('videoGeneration.skills.updatePending', { defaultValue: '更新审核' });
    }
    return t('videoGeneration.skills.publishCloud', { defaultValue: '发布到社区' });
  };

  const renderLocalList = () => {
    if (filteredRows.length === 0) {
      return (
        <div className={styles.empty}>
          {tab === 'user'
            ? t('videoGeneration.skills.emptyMine', {
                defaultValue: '还没有自己的 Skill，点下方创建',
              })
            : t('videoGeneration.skills.empty', { defaultValue: '暂无可用 Skill' })}
        </div>
      );
    }
    return filteredRows.map((skill) => {
      const active = selectedIds.includes(skill.id);
      const cloud = skill.cloud;
      const showCloudStatus = skill.source === 'user';
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
              <span className={styles.metaDiamond} aria-hidden='true' />
              {showCloudStatus ? (
                <span className={`${styles.sourceBadge} ${cloudStatusClass(cloud?.status)}`}>
                  {cloudStatusLabel(cloud?.status, t)}
                </span>
              ) : (
                <span className={styles.sourceBadge}>
                  {t('videoGeneration.skills.source.builtin', { defaultValue: '官方' })}
                </span>
              )}
              {cloud?.rejectReason && (cloud.status || '').toLowerCase() === 'offline' ? (
                <>
                  <span className={styles.metaDiamond} aria-hidden='true' />
                  <span className={styles.rejectHint} title={cloud.rejectReason}>
                    {t('videoGeneration.skills.rejected', { defaultValue: '未通过' })}
                  </span>
                </>
              ) : null}
            </small>
          </span>
          {skill.source === 'user' ? (
            <em
              className={styles.inlineAction}
              onClick={(event) => void handlePublishCloud(skill, event)}
              role='button'
              tabIndex={0}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  void handlePublishCloud(skill, event as never);
                }
              }}
            >
              {busyId === `cloud:${skill.id}` ? '…' : publishActionLabel(cloud)}
            </em>
          ) : null}
        </button>
      );
    });
  };

  const renderCloudList = () => {
    if (!cloudReady) {
      return (
        <div className={styles.authGate}>
          <p>
            {t('videoGeneration.skills.authRequired.desc', {
              defaultValue: '浏览与安装社区 Skill 需要登录云端账号。',
            })}
          </p>
          <button type='button' className={styles.footerPrimary} onClick={() => navigate('/cloud-login')}>
            {t('videoGeneration.skills.authRequired.login', { defaultValue: '去登录' })}
          </button>
        </div>
      );
    }
    if (cloudPlaza.length === 0) {
      return (
        <div className={styles.empty}>
          {t('videoGeneration.skills.cloudEmpty', { defaultValue: '社区暂无已上架 Skill' })}
        </div>
      );
    }
    return cloudPlaza.map((cloud) => {
      const installed = installedByName.get(cloud.name);
      const active = installed ? selectedIds.includes(installed.id) : false;
      return (
        <button
          key={cloud.id}
          type='button'
          role='option'
          aria-selected={active}
          className={`${homeStyles.slashMenuItem} ${styles.skillItem} ${
            active ? homeStyles.slashMenuItemActive : ''
          }`}
          onClick={() => {
            if (installed) toggle(installed.id);
          }}
        >
          <span className={styles.checkSlot} aria-hidden='true'>
            {active ? <CheckSmall size={14} /> : null}
          </span>
          <span className={styles.skillText}>
            <strong>{cloud.displayName}</strong>
            <small>
              <span className={styles.skillDesc}>{cloud.description || ''}</span>
              <span className={styles.metaDiamond} aria-hidden='true' />
              <span className={styles.sourceBadge}>
                {cloud.isMine || myCloudByName.has(cloud.name)
                  ? t('videoGeneration.skills.source.minePublished', {
                      defaultValue: '我发布的',
                    })
                  : cloud.author?.name ||
                    t('videoGeneration.skills.source.cloud', { defaultValue: '社区' })}
              </span>
            </small>
          </span>
          <em
            className={styles.inlineAction}
            onClick={(event) => void handleInstallCloud(cloud, event)}
            role='button'
            tabIndex={0}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                void handleInstallCloud(cloud, event as never);
              }
            }}
          >
            {busyId === `install:${cloud.id}`
              ? '…'
              : installed
                ? t('videoGeneration.skills.useInstalled', { defaultValue: '选用' })
                : cloud.isMine || myCloudByName.has(cloud.name)
                  ? t('videoGeneration.skills.syncMine', { defaultValue: '同步本地' })
                  : t('videoGeneration.skills.install', { defaultValue: '安装' })}
          </em>
        </button>
      );
    });
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
          ) : tab === 'hub' ? (
            renderCloudList()
          ) : (
            renderLocalList()
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
            defaultValue: '自己创建的 Skill 在「我的」；上架状态会显示审核中 / 已上架 / 已下架。',
          })}
        </p>
      </div>
    </>
  );
};

export default VerticalSkillMenu;
