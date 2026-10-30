import { Lightning } from '@icon-park/react';
import React from 'react';
import ExpertTile from '../PresetSettings/ExpertTile';
import type { MarketActionState, MarketPrimaryActionConfig } from './marketContracts';
import { formatSkillHubMarketCount } from './marketViewModel';
import type { MarketItemViewModel } from './marketViewModel';
import { normalizeTestId } from './skillPresentation';

type MarketItemTileProps = {
  item: MarketItemViewModel;
  action: MarketPrimaryActionConfig;
  actionState: MarketActionState;
  busy: boolean;
  onAction: (item: MarketItemViewModel) => void;
  onViewDetails: (item: MarketItemViewModel) => void;
};

/** Market item rendered with the same tile as the expert catalog. */
const MarketItemTile: React.FC<MarketItemTileProps> = ({
  item,
  action,
  actionState,
  busy,
  onAction,
  onViewDetails,
}) => {
  const testId = normalizeTestId(item.id);
  const downloads = item.skillHub ? formatSkillHubMarketCount(item.skillHub.downloads) : item.compactStats;

  return (
    <ExpertTile
      testId={`skill-market-card-${testId}`}
      titleTestId={`market-title-${testId}`}
      summaryTestId={`market-summary-${testId}`}
      actionTestId={`btn-add-market-skill-${testId}`}
      name={item.title}
      summary={item.summary}
      tags={item.allTags}
      actionLabel={action.label}
      actionPendingLabel={action.pendingLabel}
      actionDoneLabel={action.completedLabel}
      actionPending={busy}
      actionDone={actionState === 'completed'}
      onAction={() => onAction(item)}
      footer={
        downloads ? (
          <div className='flex h-18px items-center gap-4px text-12px leading-18px text-t-tertiary'>
            <span className='inline-flex'>
              <Lightning theme='outline' size={12} />
            </span>
            <span>{downloads}</span>
          </div>
        ) : undefined
      }
      onOpen={() => onViewDetails(item)}
    />
  );
};

export default MarketItemTile;
