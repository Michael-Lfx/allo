import type { SrtEntry } from "@oc/types/timeline";

const TERMINATORS = new Set(["。", "！", "？", "!", "?", "；", ";", "…"]);

/** Split ASR plain text into timed SRT entries using proportional duration. */
export function transcriptToSrtEntries(text: string, durationMs?: number): SrtEntry[] {
    const cleaned = text.replace(/\s+/g, " ").trim();
    if (!cleaned) return [];
    const chunks: string[] = [];
    let buffer = "";
    for (const char of cleaned) {
        buffer += char;
        if (TERMINATORS.has(char) || char === "\n") {
            const piece = buffer.trim();
            if (piece) chunks.push(piece);
            buffer = "";
        }
    }
    const tail = buffer.trim();
    if (tail) chunks.push(tail);
    const parts = chunks.length ? chunks : [cleaned];
    const weights = parts.map((chunk) => Math.max(1, chunk.length));
    const totalWeight = weights.reduce((sum, weight) => sum + weight, 0);
    const totalMs = durationMs && durationMs > 0 ? durationMs : Math.max(2000, parts.length * 2000);
    let cursor = 0;
    return parts.map((chunk, index) => {
        const span = Math.max(400, Math.round((weights[index] / totalWeight) * totalMs));
        const startMs = cursor;
        const endMs = index === parts.length - 1 ? totalMs : Math.min(totalMs, cursor + span);
        cursor = endMs;
        return { index: index + 1, startMs, endMs, text: chunk };
    });
}
