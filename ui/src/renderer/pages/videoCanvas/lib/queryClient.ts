import { QueryClient } from '@tanstack/react-query';

/** Shared QueryClient for the ported open-ai-canvas workspace. */
export const videoCanvasQueryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 30_000,
    },
  },
});
