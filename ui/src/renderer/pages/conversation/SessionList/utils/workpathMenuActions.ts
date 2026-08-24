export type WorkpathMenuAction = 'copy' | 'pin' | 'remove';

export type WorkpathMenuActionOptions = {
  isDefault: boolean;
  isProjectWorkpath: boolean;
  canRemoveProjectWorkpath: boolean;
};

/**
 * Computes the actions exposed by a workpath's compact more menu. Keeping the
 * policy independent from Arco's menu rendering makes the default-workspace
 * and project-removal boundaries directly testable.
 */
export function getWorkpathMenuActionKeys({
  isDefault,
  isProjectWorkpath,
  canRemoveProjectWorkpath,
}: WorkpathMenuActionOptions): WorkpathMenuAction[] {
  const actions: WorkpathMenuAction[] = [];

  if (!isDefault) actions.push('copy');
  actions.push('pin');
  if (!isDefault && isProjectWorkpath && canRemoveProjectWorkpath) actions.push('remove');

  return actions;
}
