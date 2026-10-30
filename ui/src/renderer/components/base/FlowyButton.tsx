/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, type ButtonProps } from '@arco-design/web-react';
import classNames from 'classnames';

export type FlowyButtonRole = 'quiet' | 'bezel' | 'ink' | 'send' | 'icon';

const ROLE_TYPE: Record<FlowyButtonRole, ButtonProps['type']> = {
  quiet: 'text',
  bezel: 'secondary',
  ink: 'primary',
  send: 'primary',
  icon: 'text',
};

type FlowyButtonProps = ButtonProps & {
  /** Visual role. Geometry stays on the shared control tokens. */
  role?: FlowyButtonRole;
};

/**
 * Thin role wrapper around Arco Button.
 * quiet: transparent row/toolbar action
 * bezel: hairline secondary
 * ink: the one default button in a dialog
 * send: accent fill, reserved for send
 * icon: 28px square symbol button
 */
const FlowyButton = ({ role = 'quiet', className, type, ...props }: FlowyButtonProps) => (
  <Button
    type={type ?? ROLE_TYPE[role]}
    {...props}
    className={classNames('flowy-btn', `flowy-btn-${role}`, role === 'icon' && 'flowy-btn-icon', className)}
  />
);

export default FlowyButton;
