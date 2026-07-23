

import React from 'react';
import OfficeWatchViewer from './OfficeWatchViewer';

interface OfficeDocPreviewProps {
  file_path?: string;
  content?: string;
  workspace?: string;
}

const OfficeDocPreview: React.FC<OfficeDocPreviewProps> = (props) => <OfficeWatchViewer docType='word' {...props} />;

export default OfficeDocPreview;
