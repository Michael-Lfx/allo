/**
 * Settings-only Markdown entry. Conversation surfaces keep the eager
 * `MarkdownView` import so chat does not pay a second Suspense hop.
 */
import React, { Suspense } from 'react';
import type { MarkdownViewProps } from './markdownViewProps';

const MarkdownView = React.lazy(() => import('./index'));

const LazyMarkdownView: React.FC<MarkdownViewProps> = (props) => (
  <Suspense
    fallback={
      <div
        aria-busy='true'
        className='min-h-48px text-12px leading-18px text-t-tertiary'
      />
    }
  >
    <MarkdownView {...props} />
  </Suspense>
);

export default LazyMarkdownView;
