export type EditResubmitResolution =
  | { kind: 'success' }
  | { kind: 'post_mutation_failure'; error: unknown };

export type EditResubmitLifecycleEvent =
  | {
      kind: 'phase';
      operationId: string;
      phase: 'submitting' | 'confirming';
      continueConfirmation?: () => void;
    }
  | {
      kind: 'terminal';
      operationId: string;
      resolution: EditResubmitResolution;
    };
