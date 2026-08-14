import { normalizeToolGroupStatus } from '@/common/chat/toolGroupStatus';
import type { ToolChipStatus } from './ToolChips';

export type ToolChipNormalizedStatus = 'pending' | 'running' | 'completed' | 'error' | 'canceled';

export const resolveToolChipStatus = ({
  status,
  skipped,
  notExecutedReason,
}: {
  status: ToolChipNormalizedStatus;
  skipped?: boolean;
  notExecutedReason?: 'invalid_arguments';
}): ToolChipStatus => {
  if (notExecutedReason === 'invalid_arguments') return 'invalid_arguments';
  if (skipped) return 'skipped';
  return status;
};

export const resolveToolChipStatusFromToolGroup = (status: unknown): ToolChipStatus => {
  const display = normalizeToolGroupStatus(status);
  switch (display) {
    case 'Success':
      return 'completed';
    case 'Error':
      return 'error';
    case 'Canceled':
      return 'canceled';
    case 'Pending':
    case 'Confirming':
      return 'pending';
    case 'Executing':
      return 'running';
    default: {
      const exhaustive: never = display;
      return exhaustive;
    }
  }
};

export type ToolChipProcessState = 'running' | 'waiting' | 'completed' | 'failed' | 'canceled';

export const resolveToolChipStatusFromProcessState = ({
  state,
  skipped,
  notExecutedReason,
}: {
  state: ToolChipProcessState;
  skipped?: boolean;
  notExecutedReason?: 'invalid_arguments';
}): ToolChipStatus => {
  if (notExecutedReason === 'invalid_arguments') return 'invalid_arguments';
  if (skipped) return 'skipped';
  switch (state) {
    case 'running':
      return 'running';
    case 'waiting':
      return 'pending';
    case 'completed':
      return 'completed';
    case 'failed':
      return 'error';
    case 'canceled':
      return 'canceled';
    default: {
      const exhaustive: never = state;
      return exhaustive;
    }
  }
};

const COMMAND_TOOL_NAMES = new Set(['bash', 'shell', 'run_commands', 'command']);

export const isCommandToolName = (name: string): boolean => COMMAND_TOOL_NAMES.has(name.trim().toLowerCase());

export const chipDetailOmittingCommand = (
  name: string,
  detail: string | undefined,
  action?: string
): string | undefined => {
  if (action === 'run_commands' || isCommandToolName(name)) return undefined;
  return detail;
};

export const resolveToolChipStatusFromAcp = (status: unknown): ToolChipStatus => {
  switch (status) {
    case 'pending':
      return 'pending';
    case 'in_progress':
      return 'running';
    case 'completed':
      return 'completed';
    case 'failed':
      return 'error';
    case 'canceled':
    case 'cancelled':
      return 'canceled';
    default:
      return 'pending';
  }
};
