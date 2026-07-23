

import React from 'react';
import OfficeWatchViewer from './OfficeWatchViewer';

interface PptViewerProps {
  file_path?: string;
  content?: string;
  workspace?: string;
}

const PptViewer: React.FC<PptViewerProps> = (props) => <OfficeWatchViewer docType='ppt' {...props} />;

export default PptViewer;
