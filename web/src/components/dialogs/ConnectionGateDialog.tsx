/**
 * 未连接时的连接门——它是**唯一**的入口状态，不是可以绕过的提示。
 *
 * 为什么不留退路：没有 App Server 时，侧栏会话、模型选择、输入框、工作区全都取不到，
 * 主页面里没有任何一件事是能做的。给一个「稍后配置」或让用户关掉对话框，就是把人放
 * 在一屏看得见、点不动的控件前面，而真正该做的那件事（填对地址并连接）反而被推到身后。
 * 所以这扇门由 `DialogShell` 的无 `onClose` 形态承担：没有关闭按钮，点外部与 Esc 都不
 * 关闭，唯二的出口是「连接成功」或「改对地址」。
 *
 * 「要不要弹」的判定在 `lib/connection-gate.ts`，不在这里：那是纯逻辑，且有容易搞错的
 * 边界（首次自动连接尚未落定），单独放是为了能直接断言。
 */

import { useTranslation } from "react-i18next";

import { shouldShowConnectionGate } from "../../lib/connection-gate";
import { useAppStore } from "../../store/appStore";
import { ConnectionSettingsSection } from "./ConnectionSettingsSection";
import { DialogShell } from "./DialogShell";

export function ConnectionGateDialog() {
  const { t } = useTranslation();
  const shouldShow = useAppStore(shouldShowConnectionGate);

  if (!shouldShow) return null;

  return (
    <DialogShell
      labelledBy="connection-gate-title"
      titleId="connection-gate-title"
      title={t("connectionGate.title")}
      width="large"
    >
      <p className="dialog-intro">{t("connectionGate.intro")}</p>
      <ConnectionSettingsSection />
    </DialogShell>
  );
}
