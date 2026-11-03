/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import LocalImageView from '@renderer/components/media/LocalImageView';
import { usePreviewLauncher } from '@/renderer/hooks/file/usePreviewLauncher';
import { getFileTypeInfo } from '@/renderer/utils/file/fileType';
import React from 'react';
import type { PinnedStreamImage } from '../streamImagePlacement';

const StreamImageBlock: React.FC<{ image: PinnedStreamImage }> = ({ image }) => {
  const { launchPreview, canPreview } = usePreviewLauncher();
  const { contentType, editable, language } = getFileTypeInfo(image.fileName);

  const picture = (
    <LocalImageView
      src={image.path}
      alt={image.fileName}
      className='block max-w-full max-h-420px object-contain rd-8px'
    />
  );

  if (!canPreview) {
    return <div className='max-w-full'>{picture}</div>;
  }

  return (
    <button
      type='button'
      className='block max-w-full cursor-pointer border-none bg-transparent p-0 text-left'
      aria-label={image.fileName}
      onClick={() => {
        void launchPreview({
          originalPath: image.path,
          file_name: image.fileName,
          contentType,
          editable,
          language,
        });
      }}
    >
      {picture}
    </button>
  );
};

export default React.memo(StreamImageBlock);
