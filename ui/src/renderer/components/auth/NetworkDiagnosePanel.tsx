/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import type { ICloudNetworkDiagnoseReport } from '@/common/adapter/ipcBridge';

function formatReport(report: ICloudNetworkDiagnoseReport): string {
  const lines = [
    `target=${report.target}`,
    `host=${report.host}`,
    `ok=${report.ok}`,
    `summary=${report.summary}`,
    ...report.steps.map(
      (step) =>
        `${step.name}: ${step.ok ? 'ok' : 'fail'} (${step.durationMs}ms) ${step.detail}`
    ),
  ];
  return lines.join('\n');
}

const NetworkDiagnosePanel: React.FC = () => {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ICloudNetworkDiagnoseReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const runDiagnose = useCallback(async () => {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const next = await ipcBridge.cloud.networkDiagnose.invoke();
      setReport(next);
    } catch (err) {
      setReport(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const copyReport = useCallback(async () => {
    if (!report) return;
    const text = formatReport(report);
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }, [report]);

  return (
    <div className='cloud-login-diagnose'>
      <button
        type='button'
        className='flowy-auth-ghost cloud-login-diagnose__trigger'
        disabled={busy}
        onClick={() => void runDiagnose()}
      >
        {busy ? t('cloudLogin.diagnose.running') : t('cloudLogin.diagnose.run')}
      </button>
      {(report || error) && (
        <div className='cloud-login-diagnose__result' role='status'>
          {error ? (
            <p className='cloud-login-diagnose__error'>{error}</p>
          ) : report ? (
            <>
              <p className={report.ok ? 'cloud-login-diagnose__ok' : 'cloud-login-diagnose__fail'}>
                {report.ok ? t('cloudLogin.diagnose.ok') : t('cloudLogin.diagnose.failed')}
                {': '}
                {report.summary}
              </p>
              <pre className='cloud-login-diagnose__pre'>{formatReport(report)}</pre>
              <button
                type='button'
                className='flowy-auth-ghost'
                onClick={() => void copyReport()}
              >
                {copied ? t('cloudLogin.diagnose.copied') : t('cloudLogin.diagnose.copy')}
              </button>
            </>
          ) : null}
        </div>
      )}
    </div>
  );
};

export default NetworkDiagnosePanel;
