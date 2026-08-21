

import classNames from 'classnames';
import React from 'react';

import { splitPath } from '@/renderer/utils/file/pathDisplay';
import MarqueeText from './MarqueeText';

type PathTextProps = {
  /** Absolute filesystem path to render. */
  path: string;
  /** Class applied to the outer container (font size / weight / color). */
  className?: string;
  /** Reveal the complete path once the pointer rests over the path. */
  marqueeOnHover?: boolean;
  /** Activate the marquee from an enclosing hover scope. */
  marqueeActive?: boolean;
};

/**
 * Renders a filesystem path with middle truncation: the parent directory
 * collapses behind an ellipsis while the final segment stays fully visible, so
 * same-named folders under different parents remain distinguishable in tight
 * widths (the sidebar, the workspace pill). The resting mode is pure CSS. The
 * optional marquee mode measures the complete raw path only while preserving
 * this split representation at rest.
 */
const PathText: React.FC<PathTextProps> = ({
  path,
  className,
  marqueeOnHover = false,
  marqueeActive = false,
}) => {
  const { head, tail } = splitPath(path);
  if (!marqueeOnHover) {
    if (!head) {
      return <span className={classNames('truncate', className)}>{tail || path}</span>;
    }
    return (
      <span className={classNames('flex items-center min-w-0 overflow-hidden', className)}>
        <span className='overflow-hidden text-ellipsis whitespace-nowrap'>{head}</span>
        <span className='shrink-0 whitespace-nowrap'>{tail}</span>
      </span>
    );
  }

  const staticContent = !head ? (
    tail || path
  ) : (
    <span className='flex items-center min-w-0 overflow-hidden'>
      <span className='overflow-hidden text-ellipsis whitespace-nowrap'>{head}</span>
      <span className='shrink-0 whitespace-nowrap'>{tail}</span>
    </span>
  );

  return (
    <MarqueeText
      text={path}
      staticContent={staticContent}
      className={className}
      active={marqueeActive}
    />
  );
};

export default PathText;
