export type EditResubmitResolution =
  | { kind: 'success' }
  | { kind: 'post_mutation_failure'; error: unknown };
