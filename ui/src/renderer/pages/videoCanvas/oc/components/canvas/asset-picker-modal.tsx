export type InsertAssetPayload =
    | { kind: "text"; content: string; title: string; assetId?: string }
    | { kind: "image"; dataUrl: string; title: string; storageKey?: string; assetId?: string }
    | { kind: "video"; url: string; title: string; storageKey?: string; width?: number; height?: number; durationMs?: number; bytes?: number; mimeType?: string; assetId?: string }
    | { kind: "audio"; url: string; title: string; storageKey?: string; durationMs?: number; bytes?: number; mimeType?: string; assetId?: string }
    | { kind: "character"; title: string; assetId: string; versionId: string; prompt: string; aliases: string[]; definition: Record<string, unknown>; coverUrl?: string; visualStatus: string; voiceStatus: string; voiceName?: string; voiceProfile?: { name: string; provider: string; language: string; timbre: string }; voiceInstructions?: string };
