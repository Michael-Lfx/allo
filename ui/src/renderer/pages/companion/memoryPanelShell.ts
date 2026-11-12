import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { tauriEvents } from '@/common/adapter/tauriEvents';
import { tauriSystem } from '@/common/adapter/tauriSystem';
import type { GeomRect } from './windowGeometry';
import { MEMORY_PANEL_LABEL } from './memoryPanelProtocol';
import type { CompanionId } from '@/common/types/ids';

export async function prepareMemoryPanelWindow(): Promise<void> {
  if (isTauriRuntime()) await tauriSystem.prepareCompanionMemoryPanel();
}
export async function placeMemoryPanelWindow(args: { requestId: string; ownerCompanionId: CompanionId; rect: GeomRect }): Promise<void> {
  if (isTauriRuntime()) await tauriSystem.placeCompanionMemoryPanel(args);
}
export async function showMemoryPanelWindow(args: { requestId: string; ownerCompanionId: CompanionId }): Promise<boolean> {
  return isTauriRuntime() ? tauriSystem.showCompanionMemoryPanel(args) : false;
}
export async function hideMemoryPanelWindow(requestId: string): Promise<boolean> {
  return isTauriRuntime() ? tauriSystem.hideCompanionMemoryPanel({ requestId }) : false;
}
export async function emitToMemoryPanel<T>(event: string, payload: T): Promise<void> {
  await tauriEvents.emitTo(MEMORY_PANEL_LABEL, event, payload);
}
export async function emitToWindow<T>(label: string, event: string, payload: T): Promise<void> {
  await tauriEvents.emitTo(label, event, payload);
}
export async function listenCurrentWindow<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  return tauriEvents.listenCurrentWindow(event, handler);
}
