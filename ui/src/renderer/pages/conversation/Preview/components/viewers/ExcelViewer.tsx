

import React from 'react';
import OfficeWatchViewer from './OfficeWatchViewer';

interface ExcelPreviewProps {
  file_path?: string;
  content?: string;
  workspace?: string;
}

const ExcelPreview: React.FC<ExcelPreviewProps> = (props) => <OfficeWatchViewer docType='excel' {...props} />;

export default ExcelPreview;
