

import type {
  AutoWorkRunState,
  IApiRobotPhase,
  IdmmRunState,
  ISshLinkPhase,
} from '@/common/adapter/ipcBridge';

import { CAPABILITY_COLORS } from './CapabilityIcon';

/**
 * Per-capability run-state → colour, derived from the shared {@link CAPABILITY_COLORS}
 * palette. This is the SINGLE routing table both surfaces read:
 *  - the conversation-header controls (AutoWorkControl / IdmmControl) colour their
 *    trigger icon + status marker through it, and
 *  - the session-list capability icons (sessionCapabilityItems) colour the row icon
 *    through it.
 * Keeping the state→colour mapping here (not re-inlined per surface) is what keeps
 * the header and the sidebar from drifting — the bug that had IDMM `off` resolve to
 * gray in the header but blue in the sidebar.
 */
export const AUTOWORK_STATUS_COLOR: Record<AutoWorkRunState, string> = {
  off: CAPABILITY_COLORS.off,
  idle: CAPABILITY_COLORS.idle,
  active: CAPABILITY_COLORS.active,
};

export const IDMM_STATUS_COLOR: Record<IdmmRunState, string> = {
  off: CAPABILITY_COLORS.off,
  armed: CAPABILITY_COLORS.armed,
  intervening: CAPABILITY_COLORS.active,
};

/** SSH link phase → colour for the conversation-header host pill. */
export const SSH_STATUS_COLOR: Record<ISshLinkPhase, string> = {
  idle: CAPABILITY_COLORS.off,
  connecting: CAPABILITY_COLORS.idle,
  connected: CAPABILITY_COLORS.active,
  degraded: CAPABILITY_COLORS.armed,
  reconnecting: CAPABILITY_COLORS.armed,
  dropped: CAPABILITY_COLORS.off,
  closed: CAPABILITY_COLORS.off,
};

/**
 * Robot phase → colour for the 机器人连接 list pill.
 *
 * `idle` is green because it means the device IS connected and waiting — for a
 * physical robot "reachable" is the good state, and `offline` (gray) is the
 * neutral absence, not a fault. `listening` / `speaking` share the primary tint
 * because the row label already says which one is happening.
 */
export const ROBOT_STATUS_COLOR: Record<IApiRobotPhase, string> = {
  offline: CAPABILITY_COLORS.off,
  idle: CAPABILITY_COLORS.active,
  listening: CAPABILITY_COLORS.primary,
  speaking: CAPABILITY_COLORS.primary,
};
