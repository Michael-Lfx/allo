/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { application, cloud } from '@/common/adapter/ipcBridge';
import type { ICloudDeviceActivationStatus } from '@/common/adapter/ipcBridge';

export type SupportLogDeviceInfo = {
  platform?: string;
  arch?: string;
  logDir?: string;
  workDir?: string;
  serialNumber?: string;
  appVersion?: string;
  activatedForVersion?: boolean;
  lastReportedIp?: string;
  collectedAt: string;
};

export async function collectSupportDeviceInfo(
  deviceStatus?: ICloudDeviceActivationStatus | null
): Promise<SupportLogDeviceInfo> {
  let system:
    | {
        platform?: string;
        arch?: string;
        logDir?: string;
        workDir?: string;
      }
    | undefined;
  try {
    system = await application.systemInfo.invoke();
  } catch {
    system = undefined;
  }

  let status = deviceStatus;
  if (status === undefined) {
    try {
      status = await cloud.deviceStatus.invoke();
    } catch {
      status = null;
    }
  }

  return {
    platform: system?.platform,
    arch: system?.arch,
    logDir: system?.logDir,
    workDir: system?.workDir,
    serialNumber: status?.serialNumber,
    appVersion: status?.appVersion,
    activatedForVersion: status?.activatedForVersion,
    lastReportedIp: status?.lastReportedIp,
    collectedAt: new Date().toISOString(),
  };
}
