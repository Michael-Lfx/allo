

import type { IChannelPairingRequest, IChannelPluginStatus } from '@/common/types/channel/channel';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import NomiModal from '@/renderer/components/base/NomiModal';
import type { ChannelPlatform } from '@/renderer/components/settings/SettingsModal/contents/channels/channelTarget';
import {
  CHANNEL_PLATFORMS,
  CREDENTIALS_REQUIRED_KEY,
  PLUGIN_DISABLED_KEY,
  PLUGIN_ENABLED_KEY,
  PlatformConfigBody,
} from '@/renderer/components/channels/PlatformConfigBody';
import {
  retargetConfigAfterStatus,
  statusOwnedBy,
  statusIsUnbound,
  type ChannelConfigTarget,
} from '@/renderer/components/channels/channelStatusSelection';
import { Button, Message, Modal, Switch, Tag } from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import type { ChannelId, CompanionId } from '@/common/types/ids';
import { useTranslation } from 'react-i18next';
import { QRCodeSVG } from 'qrcode.react';
import { useCompanions } from '../useNomi';

const { channel } = ipcBridge;

const pairingDeepLink = (code: string) => `flowy://pair?code=${encodeURIComponent(code)}`;

type ChecklistItem = {
  key: 'bound' | 'model' | 'claim' | 'pairing';
  ok: boolean;
  label: string;
};

/**
 * 伙伴设置页「远程连接」节：每伙伴视角的多机器人管理。
 * 每个机器人 = channel_plugins 一行（行上 companion_id 绑宠，UNIQUE(type,bot_key)
 * 保证同一机器人不绑多宠）。同一平台可以有多行：本宠的行直接启停/配置/解绑/
 * 删除；未绑定的行可以绑到本宠；他宠的行不可抢，但本宠可以为该平台新建自己的
 * 机器人——这是多行模型的核心能力。
 *
 * Per-companion "Remote connect" section over the multi-bot channel model. Each bot
 * is one channel_plugins row; the card for a platform branches on whether
 * this companion owns a row, an unbound row exists, or only other companions' rows exist.
 * Pending pairing requests still surface as a platform-level badge.
 */
const RemoteConnectSection: React.FC<{ companionId: CompanionId; companionName: string }> = ({ companionId, companionName }) => {
  const { t } = useTranslation();
  const { companions } = useCompanions();

  // All channel rows, indexed by row id (NOT platform type — one platform may have many rows).
  const [statuses, setStatuses] = useState<Record<string, IChannelPluginStatus>>({});
  const [pendings, setPendings] = useState<IChannelPairingRequest[]>([]);
  const [authorizedByChannel, setAuthorizedByChannel] = useState<Record<string, number>>({});
  const [modelConfigured, setModelConfigured] = useState<boolean | null>(null);
  const [busyRowId, setBusyRowId] = useState<ChannelId | null>(null);
  const [approvingCode, setApprovingCode] = useState<string | null>(null);
  // Config modal target: with channelId = edit that row; without = create mode
  // (the form's first save creates a row bound to this companion).
  const [configTarget, setConfigTarget] = useState<ChannelConfigTarget>(null);

  // ── Channel plugin statuses (REST snapshot + WS live updates) ──
  const refreshStatuses = useCallback(async () => {
    try {
      const plugins = await channel.getPluginStatus.invoke();
      if (!plugins) return;
      setStatuses(() => {
        const next: Record<string, IChannelPluginStatus> = {};
        for (const plugin of plugins) next[plugin.id] = plugin;
        return next;
      });
    } catch (error) {
      console.error('[RemoteConnect] Failed to load plugin statuses:', error);
    }
  }, []);

  useEffect(() => {
    void refreshStatuses();
    const unsubscribe = channel.pluginStatusChanged.on(({ status }) => {
      // Merge known rows by id for fast feedback, then reconcile with a REST
      // snapshot: a just-deleted row still emits one trailing enabled=false
      // event, and new rows created elsewhere only exist in the snapshot.
      setStatuses((prev) => (prev[status.id] ? { ...prev, [status.id]: { ...prev[status.id], ...status } } : prev));
      void refreshStatuses();
    });
    return () => unsubscribe();
  }, [refreshStatuses]);

  // ── Pending pairing requests (badge per channel row) + owner approve shortcut ──
  const refreshPendings = useCallback(async () => {
    try {
      const pairings = await channel.getPendingPairings.invoke();
      setPendings(pairings ?? []);
    } catch (error) {
      console.error('[RemoteConnect] Failed to load pending pairings:', error);
    }
  }, []);

  const refreshAuthorized = useCallback(async () => {
    try {
      const users = await channel.getAuthorizedUsers.invoke();
      const next: Record<string, number> = {};
      for (const user of users ?? []) {
        if (!user.channelId) continue;
        next[user.channelId] = (next[user.channelId] ?? 0) + 1;
      }
      setAuthorizedByChannel(next);
    } catch (error) {
      console.error('[RemoteConnect] Failed to load authorized users:', error);
    }
  }, []);

  const refreshModel = useCallback(async () => {
    try {
      const st = await ipcBridge.companion.getCompanionStatus.invoke({ companion_id: companionId });
      setModelConfigured(Boolean(st?.model_configured));
    } catch {
      setModelConfigured(null);
    }
  }, [companionId]);

  useEffect(() => {
    void refreshPendings();
    void refreshAuthorized();
    void refreshModel();
    const unsubs = [
      channel.pairingRequested.on(() => {
        void refreshPendings();
      }),
      channel.userAuthorized.on(() => {
        void refreshPendings();
        void refreshAuthorized();
      }),
      ipcBridge.companion.onConfigUpdated.on((evt) => {
        if (evt.scope === companionId || evt.companion_id === companionId) void refreshModel();
      }),
    ];
    return () => unsubs.forEach((unsubscribe) => unsubscribe());
  }, [refreshPendings, refreshAuthorized, refreshModel, companionId]);

  // Adopt the row created from inside a create-mode modal: once a row of that
  // platform bound to this companion shows up, retarget the modal so the enable
  // switch and the form address the new row instead of creating another one.
  useEffect(() => {
    if (!configTarget || configTarget.channelId) return;
    const created = Object.values(statuses).find(
      (s) => s.type === configTarget.platform && statusOwnedBy(s, { companionId })
    );
    if (created) setConfigTarget((prev) => retargetConfigAfterStatus(prev, created));
  }, [statuses, configTarget, companionId]);

  const companionNameOf = useCallback(
    (id: CompanionId | null | undefined) => companions.find((p) => p.id === id)?.name,
    [companions]
  );

  const pendingCounts = useMemo(() => {
    const next: Record<string, number> = {};
    for (const pairing of pendings) {
      if (!pairing.channelId) continue;
      next[pairing.channelId] = (next[pairing.channelId] ?? 0) + 1;
    }
    return next;
  }, [pendings]);

  // ── Row actions ──
  const handleToggleEnabled = useCallback(
    async (row: IChannelPluginStatus, platform: ChannelPlatform, enabled: boolean) => {
      setBusyRowId(row.id);
      try {
        if (enabled) {
          // The outer card has no credential inputs (unlike the config modal's
          // telegram token field) — point the user at the form instead.
          if (!row.hasToken) {
            Message.warning(t(CREDENTIALS_REQUIRED_KEY[platform]));
            return;
          }
          const result = await channel.enablePlugin.invoke({ plugin_id: row.id, config: {} });
          if (!result.success) {
            throw new Error(
              result.error ||
                result.message ||
                t('nomi.settings.remoteEnableFailed', { defaultValue: 'Failed to enable channel' })
            );
          }
          Message.success(t(PLUGIN_ENABLED_KEY[platform]));
        } else {
          await channel.disablePlugin.invoke({ plugin_id: row.id });
          Message.success(t(PLUGIN_DISABLED_KEY[platform]));
        }
        await refreshStatuses();
      } catch (error: unknown) {
        Message.error(error instanceof Error ? error.message : String(error));
      } finally {
        setBusyRowId(null);
      }
    },
    [refreshStatuses, t]
  );

  const handleRetry = useCallback(
    async (row: IChannelPluginStatus, platform: ChannelPlatform) => {
      setBusyRowId(row.id);
      try {
        if (!row.hasToken) {
          Message.warning(t(CREDENTIALS_REQUIRED_KEY[platform]));
          return;
        }
        const result = await channel.enablePlugin.invoke({ plugin_id: row.id, config: {} });
        if (!result.success) {
          throw new Error(
            result.error ||
              result.message ||
              t('nomi.settings.remoteRetryFailed', { defaultValue: 'Retry failed' })
          );
        }
        Message.success(t('nomi.settings.remoteRetryOk', { defaultValue: 'Channel restarted' }));
        await refreshStatuses();
      } catch (error: unknown) {
        Message.error(error instanceof Error ? error.message : String(error));
        await refreshStatuses();
      } finally {
        setBusyRowId(null);
      }
    },
    [refreshStatuses, t]
  );

  const handleApprovePairing = useCallback(
    async (code: string) => {
      setApprovingCode(code);
      try {
        await channel.approvePairing.invoke({ code });
        Message.success(t('nomi.settings.remotePairApproveOk', { defaultValue: 'Pairing approved' }));
        await refreshPendings();
        await refreshAuthorized();
      } catch (error: unknown) {
        Message.error(error instanceof Error ? error.message : String(error));
      } finally {
        setApprovingCode(null);
      }
    },
    [refreshPendings, refreshAuthorized, t]
  );

  const applyRowBinding = useCallback(
    async (rowId: ChannelId, bind: boolean) => {
      setBusyRowId(rowId);
      try {
        // Backend contract: empty companion_id clears the binding. The call atomically
        // persists the binding AND resets only this channel row's sessions.
        await channel.setChannelCompanion.invoke({ plugin_id: rowId, companion_id: bind ? companionId : null });
        Message.success(
          bind ? t('nomi.settings.remoteBindSuccess', { companionName }) : t('nomi.settings.remoteUnbindSuccess')
        );
        await refreshStatuses();
      } catch (error) {
        console.error(`[RemoteConnect] Failed to update binding for ${rowId}:`, error);
        // Conflict (bot already bound to another companion) carries the other companion's
        // name in the backend message — surface it verbatim.
        if (isBackendHttpError(error) && error.backendMessage) {
          Message.error(error.backendMessage);
        } else {
          Message.error(t('nomi.settings.remoteBindFailed'));
        }
      } finally {
        setBusyRowId(null);
      }
    },
    [companionId, companionName, refreshStatuses, t]
  );

  const confirmBind = useCallback(
    (row: IChannelPluginStatus) => {
      Modal.confirm({
        title: t('nomi.settings.remoteBindRow'),
        content: t('nomi.settings.remoteBindConfirm', { companionName }),
        onOk: () => applyRowBinding(row.id, true),
      });
    },
    [applyRowBinding, companionName, t]
  );

  const confirmUnbind = useCallback(
    (row: IChannelPluginStatus) => {
      Modal.confirm({
        title: t('nomi.settings.remoteUnbindRow'),
        content: t('nomi.settings.remoteUnbindConfirm', { companionName }),
        onOk: () => applyRowBinding(row.id, false),
      });
    },
    [applyRowBinding, companionName, t]
  );

  // Move (rebind) a bot that currently belongs to ANOTHER owner onto this
  // companion. A bot serves exactly one owner at a time, but moving is free —
  // this reuses the same setChannelCompanion rebind as bind (clears the
  // channel's old sessions server-side).
  const confirmMove = useCallback(
    (row: IChannelPluginStatus) => {
      const fromName = companionNameOf(row.companionId) ?? row.companionId ?? row.publicAgentId ?? '';
      Modal.confirm({
        title: t('nomi.settings.remoteMoveHere'),
        content: t('nomi.settings.remoteMoveConfirm', { from: fromName, to: companionName }),
        onOk: () => applyRowBinding(row.id, true),
      });
    },
    [applyRowBinding, companionNameOf, companionName, t]
  );

  const confirmDelete = useCallback(
    (row: IChannelPluginStatus) => {
      Modal.confirm({
        title: t('nomi.settings.remoteDeleteBot'),
        content: t('nomi.settings.remoteDeleteConfirm'),
        okButtonProps: { status: 'danger' },
        onOk: async () => {
          try {
            await channel.deletePlugin.invoke({ plugin_id: row.id });
            await refreshStatuses();
          } catch (error: unknown) {
            Message.error(error instanceof Error ? error.message : String(error));
          }
        },
      });
    },
    [refreshStatuses, t]
  );

  const isErrorStatus = (row: IChannelPluginStatus | null | undefined) =>
    Boolean(row && (row.status === 'error' || row.error));

  // ── Row presentation helpers ──
  const statusTag = (row: IChannelPluginStatus | null) => {
    if (!row?.hasToken) {
      return (
        <Tag size='small' color='gray'>
          {t('nomi.settings.remoteStatusNotConfigured')}
        </Tag>
      );
    }
    if (isErrorStatus(row)) {
      return (
        <Tag size='small' color='red'>
          {t('nomi.settings.remoteStatusError', { defaultValue: 'Error' })}
        </Tag>
      );
    }
    if (row.enabled && row.connected) {
      return (
        <Tag size='small' color='green'>
          {t('nomi.settings.remoteStatusRunning')}
        </Tag>
      );
    }
    if (row.enabled) {
      return (
        <Tag size='small' bordered={false} className='!bg-primary-1 !text-primary-6'>
          {t('nomi.settings.remoteStatusEnabled')}
        </Tag>
      );
    }
    return (
      <Tag size='small' color='gray'>
        {t('nomi.settings.remoteStatusDisabled')}
      </Tag>
    );
  };

  /** Bot identity line (botUsername preferred over raw botKey), empty when unknown. */
  const botIdentityOf = (row: IChannelPluginStatus | null) => {
    const bot = row?.botUsername || row?.botKey;
    return bot ? t('nomi.settings.remoteBotIdentity', { bot }) : '';
  };

  const checklistFor = (row: IChannelPluginStatus | null): ChecklistItem[] => {
    const bound = Boolean(row && statusOwnedBy(row, { companionId }));
    const modelOk = modelConfigured === true;
    const claimOk = Boolean(row?.enabled && row.connected && !isErrorStatus(row));
    const pairingOk = Boolean(row && (authorizedByChannel[row.id] ?? 0) > 0);
    return [
      {
        key: 'bound',
        ok: bound,
        label: t('nomi.settings.remoteCheckBound', { defaultValue: 'Companion bound' }),
      },
      {
        key: 'model',
        ok: modelOk,
        label: t('nomi.settings.remoteCheckModel', { defaultValue: 'Chat model configured' }),
      },
      {
        key: 'claim',
        ok: claimOk,
        label: t('nomi.settings.remoteCheckClaim', { defaultValue: 'Channel / Gateway ready' }),
      },
      {
        key: 'pairing',
        ok: pairingOk,
        label: t('nomi.settings.remoteCheckPairing', { defaultValue: 'At least one paired user' }),
      },
    ];
  };

  const allRows = useMemo(() => Object.values(statuses), [statuses]);

  const configChannel = useMemo(
    () => CHANNEL_PLATFORMS.find((p) => p.id === configTarget?.platform),
    [configTarget?.platform]
  );

  const myOwnedRows = useMemo(
    () => allRows.filter((s) => s.hasToken && statusOwnedBy(s, { companionId })),
    [allRows, companionId]
  );

  const ownerPendings = useMemo(() => {
    const myIds = new Set(myOwnedRows.map((r) => r.id));
    return pendings.filter((p) => p.channelId && myIds.has(p.channelId));
  }, [pendings, myOwnedRows]);

  return (
    <>
      <div className='mt-8px text-13px font-600 text-t-secondary'>{t('nomi.settings.remoteTitle')}</div>
      <div className='text-12px text-t-tertiary -mt-6px'>{t('nomi.settings.remoteHint', { companionName })}</div>

      {ownerPendings.length > 0 && (
        <div className='mt-10px flex flex-col gap-10px bg-fill-2 rd-10px px-14px py-12px'>
          <div className='text-13px font-600 text-t-primary'>
            {t('nomi.settings.remoteSelfPairTitle', { defaultValue: 'Approve your pairing' })}
          </div>
          <div className='text-12px text-t-tertiary'>
            {t('nomi.settings.remoteSelfPairHint', {
              defaultValue: 'Scan the QR or tap Approve — no need to paste the 6-digit code.',
            })}
          </div>
          {ownerPendings.map((pairing) => (
            <div key={pairing.code} className='flex items-center gap-14px flex-wrap'>
              <QRCodeSVG value={pairingDeepLink(pairing.code)} size={96} level='M' />
              <div className='flex flex-col gap-6px min-w-0'>
                <div className='text-13px text-t-primary truncate'>
                  {pairing.display_name || pairing.platformUserId}
                  <span className='text-t-tertiary text-12px ml-6px'>({pairing.platformType})</span>
                </div>
                <code className='text-12px text-t-secondary'>{pairing.code}</code>
                <Button
                  size='small'
                  type='primary'
                  loading={approvingCode === pairing.code}
                  onClick={() => void handleApprovePairing(pairing.code)}
                >
                  {t('nomi.settings.remotePairApprove', { defaultValue: 'Approve' })}
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {CHANNEL_PLATFORMS.map(({ id, logo, titleKey, fallback }) => {
        const title = t(titleKey, fallback);
        // Only real DB rows: `GET /plugins` pads every builtin platform with
        // a placeholder entry (id == platform name, hasToken=false) when it
        // has no rows yet. Real rows always carry an encrypted config
        // (hasToken=true) — without this filter an empty platform would be
        // misread as "an unbound bot exists" and offer a binding that 404s.
        const rows = allRows.filter((s) => s.type === id && s.hasToken);
        const myRow = rows.find((r) => statusOwnedBy(r, { companionId }));
        const unboundRows = rows.filter((r) => statusIsUnbound(r));
        const otherRows = rows.filter((r) => !statusIsUnbound(r) && !statusOwnedBy(r, { companionId }));
        // The row this card talks about: this companion's bot, else a bindable one.
        const focusRow = myRow ?? unboundRows[0] ?? null;
        // Pending-pairing badge is per channel row (keyed by channelId), so a
        // second bot of the same platform shows its own count, not the platform's.
        const pending = focusRow ? (pendingCounts[focusRow.id] ?? 0) : 0;
        const checklist = myRow ? checklistFor(myRow) : null;
        const showChecklist = Boolean(myRow && checklist?.some((item) => !item.ok));

        let subtitle = '';
        let actions: React.ReactNode;
        if (myRow) {
          subtitle = botIdentityOf(myRow);
          actions = (
            <>
              <Switch
                checked={myRow.enabled}
                loading={busyRowId === myRow.id}
                onChange={(checked: boolean) => void handleToggleEnabled(myRow, id, checked)}
              />
              {isErrorStatus(myRow) && (
                <Button
                  size='small'
                  type='primary'
                  status='danger'
                  loading={busyRowId === myRow.id}
                  onClick={() => void handleRetry(myRow, id)}
                >
                  {t('nomi.settings.remoteRetry', { defaultValue: 'Retry' })}
                </Button>
              )}
              <Button size='small' onClick={() => setConfigTarget({ platform: id, channelId: myRow.id })}>
                {t('nomi.settings.remoteConfigure')}
              </Button>
              <Button size='small' onClick={() => confirmUnbind(myRow)}>
                {t('nomi.settings.remoteUnbindRow')}
              </Button>
              <Button size='small' status='danger' onClick={() => confirmDelete(myRow)}>
                {t('nomi.settings.remoteDeleteBot')}
              </Button>
            </>
          );
        } else if (unboundRows.length > 0) {
          const bindable = unboundRows[0];
          subtitle = [t('nomi.settings.remoteUnboundBot'), botIdentityOf(bindable)].filter(Boolean).join(' · ');
          actions = (
            <>
              <Button
                size='small'
                type='primary'
                loading={busyRowId === bindable.id}
                onClick={() => confirmBind(bindable)}
              >
                {t('nomi.settings.remoteBindRow')}
              </Button>
              <Button size='small' onClick={() => setConfigTarget({ platform: id, channelId: bindable.id })}>
                {t('nomi.settings.remoteConfigure')}
              </Button>
            </>
          );
        } else if (otherRows.length > 0) {
          const movable = otherRows[0];
          subtitle = t('nomi.settings.remoteOtherBots', {
            num: otherRows.length,
            companions: otherRows.map((r) => companionNameOf(r.companionId) ?? r.companionId).join(', '),
          });
          actions = (
            <>
              <Button size='small' type='primary' loading={busyRowId === movable.id} onClick={() => confirmMove(movable)}>
                {t('nomi.settings.remoteMoveHere')}
              </Button>
              <Button size='small' onClick={() => setConfigTarget({ platform: id })}>
                {t('nomi.settings.remoteCreateBot')}
              </Button>
            </>
          );
        } else {
          actions = (
            <Button size='small' type='primary' onClick={() => setConfigTarget({ platform: id })}>
              {t('nomi.settings.remoteCreateBot')}
            </Button>
          );
        }

        return (
          <div key={id} className='flex flex-col gap-8px bg-fill-2 rd-10px px-14px py-12px'>
            <div className='flex items-center gap-16px flex-wrap'>
              <div className='flex items-center gap-10px w-200px shrink-0 min-w-0'>
                <img src={logo} alt={title} className='w-18px h-18px object-contain shrink-0' />
                <div className='min-w-0'>
                  <div className='flex items-center gap-6px'>
                    <span className='text-14px text-t-primary font-500 truncate'>{title}</span>
                    {statusTag(focusRow)}
                  </div>
                  {pending > 0 && (
                    <Tag size='small' color='orangered' className='mt-4px'>
                      {t('nomi.settings.remotePending', { num: pending })}
                    </Tag>
                  )}
                </div>
              </div>
              <div className='flex-1 min-w-0 text-12px text-t-tertiary'>{subtitle}</div>
              <div className='flex items-center gap-8px shrink-0'>{actions}</div>
            </div>
            {isErrorStatus(myRow) && myRow?.error && (
              <div className='text-12px text-[rgb(var(--danger-6))] break-all'>
                {t('nomi.settings.remoteErrorDetail', { defaultValue: 'Error' })}: {myRow.error}
              </div>
            )}
            {showChecklist && checklist && (
              <div className='flex flex-col gap-4px pt-2px border-t border-border-2'>
                <div className='text-12px font-500 text-t-secondary'>
                  {t('nomi.settings.remoteChecklistTitle', { defaultValue: 'First-message checklist' })}
                </div>
                <div className='flex flex-wrap gap-8px'>
                  {checklist.map((item) => (
                    <Tag key={item.key} size='small' color={item.ok ? 'green' : 'orangered'}>
                      {item.ok ? 'OK' : '!'} {item.label}
                    </Tag>
                  ))}
                </div>
              </div>
            )}
          </div>
        );
      })}

      <NomiModal
        visible={Boolean(configTarget)}
        onCancel={() => {
          setConfigTarget(null);
          // Pairings may have been approved/rejected inside the form.
          void refreshPendings();
          void refreshStatuses();
          void refreshAuthorized();
        }}
        header={{
          title: t('nomi.settings.remoteConfigTitle', {
            channel: configChannel ? t(configChannel.titleKey, configChannel.fallback) : '',
          }),
          showClose: true,
        }}
        footer={null}
        style={{ width: 720 }}
        contentStyle={{ maxHeight: 'calc(80vh - 80px)', padding: '0 2px' }}
      >
        {configTarget && (
          <PlatformConfigBody
            key={configTarget.channelId ?? `${configTarget.platform}:new`}
            platform={configTarget.platform}
            status={configTarget.channelId ? (statuses[configTarget.channelId] ?? null) : null}
            channelTarget={{
              channelId: configTarget.channelId,
              companionId,
            }}
            onStatusChange={(status) => {
              // Forms report the row they saved; merge by row id, then let the
              // snapshot reconcile (create mode discovers the new row there).
              if (status) {
                setStatuses((prev) => ({ ...prev, [status.id]: status }));
                setConfigTarget((prev) => retargetConfigAfterStatus(prev, status));
              }
              void refreshStatuses();
            }}
            refreshStatuses={refreshStatuses}
          />
        )}
      </NomiModal>
    </>
  );
};

export default RemoteConnectSection;
