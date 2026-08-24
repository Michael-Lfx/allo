import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { useDisclosureMotion } from './useDisclosureMotion';

const MotionProbe: React.FC<{ open: boolean }> = ({ open }) => {
  const motion = useDisclosureMotion(open, 0);

  return (
    <output
      data-phase={motion.phase}
      data-should-render={String(motion.shouldRender)}
    />
  );
};

describe('useDisclosureMotion initial render', () => {
  test('renders open disclosures immediately', () => {
    const markup = renderToStaticMarkup(<MotionProbe open />);

    expect(markup).toContain('data-phase="open"');
    expect(markup).toContain('data-should-render="true"');
  });

  test('does not mount closed disclosures before a user toggle', () => {
    const markup = renderToStaticMarkup(<MotionProbe open={false} />);

    expect(markup).toContain('data-phase="closed"');
    expect(markup).toContain('data-should-render="false"');
  });
});
