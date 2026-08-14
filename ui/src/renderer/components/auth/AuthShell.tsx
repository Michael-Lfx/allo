/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { type ReactNode } from 'react';
import IntentField from './IntentField';
import type {
  BlueprintRouteStep,
  CloudActivationLevel,
  IntentFieldMode,
  IntentFieldPhase,
} from './authTypes';

export interface AuthShellProps {
  mode: IntentFieldMode;
  phase: IntentFieldPhase;
  activationLevel?: CloudActivationLevel;
  inputEnergy?: number;
  blueprintStep?: BlueprintRouteStep;
  brandTitle: ReactNode;
  brandTagline: ReactNode;
  brandFlow?: ReactNode;
  brandLogo?: ReactNode;
  brandMeta?: ReactNode;
  brandLegal?: ReactNode;
  windowControls?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}

const AuthShell: React.FC<AuthShellProps> = ({
  mode,
  phase,
  activationLevel = 0,
  inputEnergy = 0,
  blueprintStep,
  brandTitle,
  brandTagline,
  brandFlow,
  brandLogo,
  brandMeta,
  brandLegal,
  windowControls,
  children,
  footer,
  className,
}) => (
  <div className={`flowy-auth-page${className ? ` ${className}` : ''}`}>
    {windowControls && (
      <div className='flowy-auth-chrome' data-tauri-drag-region>
        {windowControls}
      </div>
    )}
    <div className='flowy-auth-shell'>
      <aside className='flowy-auth-brand'>
        <IntentField
          mode={mode}
          phase={phase}
          activationLevel={activationLevel}
          inputEnergy={inputEnergy}
          blueprintStep={blueprintStep}
          className='flowy-auth-intent'
        >
          <div className='flowy-auth-brand__lockup'>
            <div className='flowy-auth-brand__lockup-row'>
              {brandLogo && (
                <div className='flowy-auth-brand__logo' aria-hidden='true'>
                  {brandLogo}
                </div>
              )}
              <h2 className='flowy-auth-brand__wordmark'>{brandTitle}</h2>
            </div>
          </div>
          <div className='flowy-auth-brand__copy'>
            <p className='flowy-auth-brand__tagline'>{brandTagline}</p>
            {brandFlow && <p className='flowy-auth-brand__flow'>{brandFlow}</p>}
            {brandMeta && <div className='flowy-auth-brand__meta'>{brandMeta}</div>}
            {brandLegal && <div className='flowy-auth-brand__legal'>{brandLegal}</div>}
          </div>
        </IntentField>
      </aside>
      <main className='flowy-auth-panel'>
        <div className='flowy-auth-panel__inner'>
          <div className='flowy-auth-panel__content'>{children}</div>
          {footer && <div className='flowy-auth-footer'>{footer}</div>}
        </div>
      </main>
    </div>
  </div>
);

export default AuthShell;
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */
