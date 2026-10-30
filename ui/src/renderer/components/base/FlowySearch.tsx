/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Input, type InputProps } from '@arco-design/web-react';
import type { RefInputType } from '@arco-design/web-react/es/Input';
import classNames from 'classnames';
import React from 'react';
import { FlowyIcon } from './FlowyIcon';

type FlowySearchProps = InputProps & {
  /** Toolbar fields are 32px. Sidebar and list filters are 28px. */
  density?: 'toolbar' | 'compact';
};

/**
 * One search field. Leading magnifier, clear only once there is text,
 * 13px regular type, 8px corners. Placement stays with the caller.
 */
const FlowySearch = React.forwardRef<RefInputType, FlowySearchProps>(
  ({ className, density = 'toolbar', prefix, allowClear = true, ...props }, ref) => (
    <Input
      ref={ref}
      allowClear={allowClear}
      {...props}
      prefix={prefix ?? <FlowyIcon name='search' size={14} />}
      className={classNames('flowy-search', density === 'compact' && 'flowy-search--compact', className)}
    />
  )
);

FlowySearch.displayName = 'FlowySearch';

export default FlowySearch;
