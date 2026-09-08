/**
 * Ordinary Skills have one authoritative market source: SkillHub. MCP,
 * expert-package, and other capability markets keep their own panels.
 */
import React from 'react';
import SkillHubMarketPanel from './SkillHubMarketPanel';

type SkillMarketSettingsProps = {
  active?: boolean;
  hideSearch?: boolean;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
};

const SkillMarketSettings: React.FC<SkillMarketSettingsProps> = (props) => (
  <div className='w-full pb-16px'>
    <SkillHubMarketPanel {...props} />
  </div>
);

export default SkillMarketSettings;
