/**
 * Gate persisted waiting items until the host model catalog has actually
 * synced. A failed or in-flight catalog must not look "ready": the default
 * empty config would turn recoverable work into permanent failures.
 */
export function canScheduleCanvasGenerationBatches(
  projectLoaded: boolean,
  modelCatalogReady: boolean
): boolean {
  return projectLoaded && modelCatalogReady;
}
