import { Collapse } from '@arco-design/web-react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';

import CopyIconButton from './CopyIconButton';
import {
  formatErrorDiagnosticText,
  getErrorDiagnosticLabels,
  type SafeErrorDiagnostic,
} from '@/renderer/utils/ui/errorDiagnostics';

type ErrorDiagnosticContentProps = {
  diagnostic: SafeErrorDiagnostic;
  className?: string;
};

/**
 * A compact, progressively disclosed diagnostic surface for Modal content.
 * The formatter has already removed sensitive values; this component only
 * renders the safe summary/metadata and the full diagnostic text.
 */
const ErrorDiagnosticContent: React.FC<ErrorDiagnosticContentProps> = ({ diagnostic, className }) => {
  const { t } = useTranslation();
  const labels = getErrorDiagnosticLabels((key) => t(key));
  const summary = diagnostic.summary || t('conversation.agentError.unknownError');
  const displayDiagnostic = diagnostic.summary ? diagnostic : { ...diagnostic, summary };
  const diagnosticText = formatErrorDiagnosticText(displayDiagnostic, labels);

  return (
    <div className={classNames('conversation-error-diagnostic', className)}>
      <div className='conversation-error-diagnostic__header'>
        <div className='conversation-error-diagnostic__meta'>
          {diagnostic.modelId ? (
            <span className='conversation-error-diagnostic__meta-item conversation-error-diagnostic__meta-item--model'>
              <span className='conversation-error-diagnostic__meta-label'>
                {t('conversation.agentError.modelId')}
              </span>
              <code title={diagnostic.modelId}>{diagnostic.modelId}</code>
            </span>
          ) : null}
          {diagnostic.code ? (
            <span className='conversation-error-diagnostic__meta-item conversation-error-diagnostic__meta-item--code'>
              <span className='conversation-error-diagnostic__meta-label'>
                {t('conversation.agentError.errorCode')}
              </span>
              <code>{diagnostic.code}</code>
            </span>
          ) : null}
          {diagnostic.status !== undefined ? (
            <span className='conversation-error-diagnostic__meta-item conversation-error-diagnostic__meta-item--status'>
              <span className='conversation-error-diagnostic__meta-label'>
                {t('conversation.agentError.httpStatus')}
              </span>
              <code>{diagnostic.status}</code>
            </span>
          ) : null}
        </div>
        <CopyIconButton
          text={diagnosticText}
          tooltip={t('conversation.agentError.copyDiagnostic')}
          successMessage={t('conversation.agentError.copyDiagnosticSuccess')}
          className='conversation-error-diagnostic__copy'
        />
      </div>
      <div className='conversation-error-diagnostic__summary'>{summary}</div>
      {diagnosticText ? (
        <Collapse bordered={false} className='conversation-error-diagnostic__details'>
          <Collapse.Item name='technical-details' header={t('common.technical_details')}>
            <pre className='conversation-error-diagnostic__detail'>{diagnosticText}</pre>
          </Collapse.Item>
        </Collapse>
      ) : null}
    </div>
  );
};

export default ErrorDiagnosticContent;
