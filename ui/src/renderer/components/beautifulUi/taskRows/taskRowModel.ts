import type { TaskRowStatus } from './TaskRows';

export type TaskRowProcessState = 'running' | 'waiting' | 'completed' | 'failed' | 'canceled';

export const resolveTaskRowStatusFromProcessState = (state: TaskRowProcessState): TaskRowStatus => {
  switch (state) {
    case 'running':
      return 'running';
    case 'waiting':
      return 'waiting';
    case 'completed':
      return 'completed';
    case 'failed':
      return 'failed';
    case 'canceled':
      return 'canceled';
    default: {
      const exhaustive: never = state;
      return exhaustive;
    }
  }
};

export const resolveTaskGroupStatus = (state: TaskRowProcessState): TaskRowStatus => {
  if (state === 'failed') return 'completed';
  return resolveTaskRowStatusFromProcessState(state);
};
