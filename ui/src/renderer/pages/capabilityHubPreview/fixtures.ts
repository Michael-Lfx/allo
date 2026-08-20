export const HUB_IDS = ['presets', 'skills', 'mcp', 'plugins'] as const;
export const CARD_IDS = ['1', '2', '3', '4', '5', '6'] as const;

export type HubId = (typeof HUB_IDS)[number];
export type HubView = 'market' | 'installed';
export type HubPreviewVariant = 'now' | 'proposed';

export const INSTALLED_CARD_IDS = ['1', '2', '3'] as const;
