

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Preset Config One MCP', () => {
  test('editor persists mcp_server_ids and Guid omits override for preset launches', () => {
    const types = readSource(new URL('../../../../common/types/agent/presetTypes.ts', import.meta.url));
    const drawer = readSource(new URL('./PresetEditDrawer.tsx', import.meta.url));
    const editorHook = readSource(new URL('../../../hooks/preset/usePresetEditor.ts', import.meta.url));
    const guidSend = readSource(new URL('../../guid/hooks/useGuidSend.ts', import.meta.url));

    expect(types.includes('mcp_server_ids: string[]')).toBe(true);
    expect(drawer.includes("t('settings.presetMcp'")).toBe(true);
    expect(drawer.includes('setMcpServerIds')).toBe(true);
    expect(editorHook.includes('mcp_server_ids: mcpServerIds.map(String)')).toBe(true);
    expect(guidSend.includes('presetUsesSnapshotMcp')).toBe(true);
    expect(guidSend.includes('selected_mcp_server_ids: selectedUserMcpServerIds')).toBe(true);
  });
});
