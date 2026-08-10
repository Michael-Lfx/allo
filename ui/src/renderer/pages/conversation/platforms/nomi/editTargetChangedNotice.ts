export type EditTargetChangedNoticeEvent =
  | 'target_changed'
  | 'dismissed'
  | 'operation_started'
  | 'conversation_changed';

/** Keep the stale-target notice scoped to the current composer lifecycle. */
export const resolveEditTargetChangedNotice = (
  _current: boolean,
  event: EditTargetChangedNoticeEvent
): boolean => event === 'target_changed';
