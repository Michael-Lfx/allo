export interface SingleFlightRef<T> {
  current: Promise<T> | null;
}

export const runSingleFlight = <T>(
  ref: SingleFlightRef<T>,
  operation: () => Promise<T>
): Promise<T> => {
  if (ref.current) return ref.current;
  const promise = operation().finally(() => {
    if (ref.current === promise) ref.current = null;
  });
  ref.current = promise;
  return promise;
};
