

import { useCallback, useEffect, useState } from 'react';
import { application, systemSettings } from '@/common/adapter/ipcBridge';
import { configService } from '@/common/config/configService';
import {
  applyKeepAwakeSetting,
  KEEP_AWAKE_CONFIG_KEY,
  LEGACY_KEEP_AWAKE_CONFIG_KEY,
  readKeepAwakeConfig,
} from './keepAwakeSetting';

// 默认关闭:未持久化时不主动阻止休眠 / Keep-awake defaults to OFF when unset.
const readKeepAwake = (): boolean => readKeepAwakeConfig((key) => configService.get(key));

// A failed PUT may have been accepted by the backend before the client saw a
// transport error. Re-read the authoritative snapshot before deciding which
// native/local value to restore.
const readPersistedKeepAwake = async (): Promise<boolean> => {
  await configService.reload();
  if (!configService.isInitialized()) {
    throw new Error('Keep-awake preference readback failed');
  }
  return readKeepAwake();
};

/**
 * 共享的"保持唤醒"状态。toggle 时:乐观更新 -> 应用 OS 效果(applyKeepAwake)-> 持久化(HTTP PUT)。
 * 定时任务页与「设置->系统」都用本 hook,经 configService.subscribe 自动双向同步。
 */
export function useKeepAwake(): { keepAwake: boolean; setKeepAwake: (enabled: boolean) => Promise<void> } {
  const [keepAwake, setKeepAwakeState] = useState<boolean>(readKeepAwake);

  useEffect(() => {
    let active = true;
    const syncKeepAwake = () => setKeepAwakeState(readKeepAwake());
    const unsubCanonical = configService.subscribe(KEEP_AWAKE_CONFIG_KEY, syncKeepAwake);
    const unsubLegacy = configService.subscribe(LEGACY_KEEP_AWAKE_CONFIG_KEY, syncKeepAwake);
    void configService.whenReady().then(() => {
      if (active) setKeepAwakeState(readKeepAwake());
    });
    return () => {
      active = false;
      unsubCanonical();
      unsubLegacy();
    };
  }, []);

  const setKeepAwake = useCallback(async (enabled: boolean) => {
    await applyKeepAwakeSetting(enabled, {
      getCurrent: readKeepAwake,
      setLocal: (value) => configService.setLocal(KEEP_AWAKE_CONFIG_KEY, value),
      applyNative: (value) => application.applyKeepAwake.invoke({ enabled: value }),
      persist: (value) => systemSettings.setKeepAwake.invoke({ enabled: value }),
      readPersisted: readPersistedKeepAwake,
    });
  }, []);

  return { keepAwake, setKeepAwake };
}
