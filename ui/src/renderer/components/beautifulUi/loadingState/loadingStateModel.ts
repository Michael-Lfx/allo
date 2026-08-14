export const formatLoadingElapsed = (seconds: number): string =>
  `${Math.max(0, Math.floor(seconds))}s`;
