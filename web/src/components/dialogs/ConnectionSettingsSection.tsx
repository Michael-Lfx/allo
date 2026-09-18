/**
 * 连接配置区块：`App Server` 地址 / 令牌 / 连接状态与连接动作。
 *
 * 单独成文件的原因：同一组配置现在有**两个**入口——设置对话框的「通用」分区，
 * 以及未连接时挡在主界面前面的连接对话框。两者渲染的是同一份状态、同一批
 * `connect` / `disconnect` 动作，只有底部动作区不同（设置里只连，门里还要给一个
 * 「稍后配置」的出口），所以按 `ProviderSettingsSection` 的既有做法拆成：
 * - `ConnectionSettingsView`：纯 props 的展示层，可脱离 store 渲染；
 * - `ConnectionSettingsSection`：接上 `appStore` 的薄壳。
 *
 * 失败必须可见：连接失败的原文来自 store 的 `error`（宿主自己的话），直接原样显示在
 * 输入框下方——没有它，用户在一个连不上的连接门里看不到任何原因。
 */

import { useTranslation } from "react-i18next";

import { isConnectionFailureKey } from "../../lib/connect-error";
import { useAppStore } from "../../store/appStore";
import type { ConnectionPhase } from "../../ui/connection";

export interface ConnectionSettingsViewProps {
  wsUrl: string;
  token: string;
  phase: ConnectionPhase;
  /** 最近一次连接失败的原文；`null` = 没有失败信息。 */
  error: string | null;
  onWsUrlChange: (value: string) => void;
  onTokenChange: (value: string) => void;
  onConnect: () => void;
  onDisconnect: () => void;
  /**
   * 连接中时是否禁用输入。连接期间保留可编辑没有意义（改地址不会重连），避开
   * 「输入已改、连接仍在旧地址上」的错觉。
   */
  busy?: boolean;
}

/** 纯展示：无 store、无 client、无隐藏状态。 */
export function ConnectionSettingsView({
  wsUrl,
  token,
  phase,
  error,
  onWsUrlChange,
  onTokenChange,
  onConnect,
  onDisconnect,
  busy = false,
}: ConnectionSettingsViewProps) {
  const { t } = useTranslation();
  const connected = phase === "online";
  const connecting = phase === "connecting";
  const disabled = busy || connecting;
  const canConnect = !disabled && wsUrl.trim().length > 0;

  return (
    <>
      <div className="settings-card">
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.wsUrl")}</span>
            <span className="settings-row-desc">{t("settings.wsUrlDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            value={wsUrl}
            onChange={(event) => onWsUrlChange(event.target.value)}
            spellCheck={false}
            autoComplete="url"
            disabled={disabled}
            aria-label={t("settings.wsUrl")}
          />
        </div>
        <div className="settings-row">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.token")} <small>{t("settings.optional")}</small></span>
            <span className="settings-row-desc">{t("settings.tokenDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            type="password"
            value={token}
            onChange={(event) => onTokenChange(event.target.value)}
            autoComplete="current-password"
            disabled={disabled}
            aria-label={t("settings.token")}
          />
        </div>
        <div className="settings-row settings-row-status">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("settings.connectionStatus")}</span>
            <span className={`settings-row-desc ${error && !connected ? "is-error" : ""}`}>
              {connected || connecting
                ? connected ? t("settings.connected") : t("settings.connecting")
                : error
                  ? isConnectionFailureKey(error) ? t(error) : error
                  : t("settings.disconnected")}
            </span>
          </div>
          <div className="settings-row-actions">
            {connected && <button className="quiet-button" type="button" onClick={onDisconnect}>{t("settings.disconnect")}</button>}
            {!connected && (
              <button className="primary-button" type="button" onClick={onConnect} disabled={!canConnect}>
                {connecting ? t("settings.connectingBtn") : t("settings.connect")}
              </button>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

/** 接上 store 的薄壳：设置分区与连接门共用。 */
export function ConnectionSettingsSection() {
  const wsUrl = useAppStore((s) => s.wsUrl);
  const token = useAppStore((s) => s.token);
  const phase = useAppStore((s) => s.phase);
  const error = useAppStore((s) => s.error);
  const setWsUrl = useAppStore((s) => s.setWsUrl);
  const setToken = useAppStore((s) => s.setToken);
  const connect = useAppStore((s) => s.connect);
  const disconnect = useAppStore((s) => s.disconnect);

  return (
    <ConnectionSettingsView
      wsUrl={wsUrl}
      token={token}
      phase={phase}
      error={error}
      onWsUrlChange={setWsUrl}
      onTokenChange={setToken}
      onConnect={() => void connect()}
      onDisconnect={disconnect}
    />
  );
}
