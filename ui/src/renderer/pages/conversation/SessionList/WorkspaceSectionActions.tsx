import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import { ExpandDownOne, FoldUpOne, FolderPlus } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';

export type WorkspaceSectionActionsProps = {
  expanded: boolean;
  dropdownOpen: boolean;
  onToggleExpanded: () => void;
  onToggleDropdown: () => void;
  dropdownTriggerRef: React.RefObject<HTMLButtonElement | null>;
  expandLabel: string;
  collapseLabel: string;
  addLabel: string;
  className?: string;
};

/**
 * Icon-only actions for the workspace section heading.
 *
 * The icons describe the two actions without taking space away from the
 * workspace label. The hover/focus tooltip keeps the controls discoverable,
 * while the button labels remain available to assistive technology.
 */
const WorkspaceSectionActions: React.FC<WorkspaceSectionActionsProps> = ({
  expanded,
  dropdownOpen,
  onToggleExpanded,
  onToggleDropdown,
  dropdownTriggerRef,
  expandLabel,
  collapseLabel,
  addLabel,
  className,
}) => {
  const disclosureLabel = expanded ? collapseLabel : expandLabel;

  return (
    <div className={classNames('flowy-workspace-section-actions', className)}>
      <InstantHoverTooltip content={disclosureLabel} position='top'>
        <button
          type='button'
          aria-label={disclosureLabel}
          aria-expanded={expanded}
          aria-controls='flowy-workpath-tree'
          onClick={onToggleExpanded}
          className='flowy-workspace-section-action'
        >
          {expanded ? (
            <FoldUpOne theme='outline' size='12' fill='currentColor' />
          ) : (
            <ExpandDownOne theme='outline' size='12' fill='currentColor' />
          )}
        </button>
      </InstantHoverTooltip>

      <InstantHoverTooltip content={addLabel} position='top'>
        <button
          ref={dropdownTriggerRef}
          type='button'
          aria-label={addLabel}
          aria-expanded={dropdownOpen}
          aria-haspopup='dialog'
          onClick={onToggleDropdown}
          className={classNames('flowy-workspace-section-action', dropdownOpen && 'is-active')}
        >
          <FolderPlus theme='outline' size='12' fill='currentColor' />
        </button>
      </InstantHoverTooltip>
    </div>
  );
};

export default WorkspaceSectionActions;
