import type { SrtEntry } from "@oc/types/timeline";

function timeToMs(timestamp: string): number {
    const [hours, minutes, secondsAndMs] = timestamp.split(":");
    const [seconds, milliseconds] = secondsAndMs.split(",");

    return Number.parseInt(hours, 10) * 3_600_000 + Number.parseInt(minutes, 10) * 60_000 + Number.parseInt(seconds, 10) * 1_000 + Number.parseInt(milliseconds, 10);
}

export function parseSrt(content: string): SrtEntry[] {
    if (!content.trim()) {
        return [];
    }

    const blocks = content.trim().split(/\r?\n\s*\r?\n/);
    const entries: SrtEntry[] = [];

    for (const block of blocks) {
        const lines = block.trim().split(/\r?\n/);
        if (lines.length < 3) {
            continue;
        }

        const index = Number.parseInt(lines[0], 10);
        if (Number.isNaN(index)) {
            continue;
        }

        const match = lines[1].match(/\d{2}:\d{2}:\d{2},\d{3}\s*-->\s*\d{2}:\d{2}:\d{2},\d{3}/);
        if (!match) {
            continue;
        }

        const [start, end] = match[0].split(/\s*-->\s*/);
        entries.push({
            index,
            startMs: timeToMs(start),
            endMs: timeToMs(end),
            text: lines.slice(2).join("\n"),
        });
    }

    return entries;
}

function msToTimestamp(ms: number): string {
    const safeMs = Math.max(0, Math.round(ms));
    const hours = Math.floor(safeMs / 3_600_000);
    const minutes = Math.floor((safeMs % 3_600_000) / 60_000);
    const seconds = Math.floor((safeMs % 60_000) / 1_000);
    const milliseconds = safeMs % 1_000;
    return `${String(hours).padStart(2, "0")}:` + `${String(minutes).padStart(2, "0")}:` + `${String(seconds).padStart(2, "0")},` + `${String(milliseconds).padStart(3, "0")}`;
}

export function serializeSrtEntries(entries: SrtEntry[]): string {
    if (entries.length === 0) {
        return "";
    }
    return (
        entries
            .map((entry, idx) => {
                const index = Number.isInteger(entry.index) && entry.index > 0 ? entry.index : idx + 1;
                return `${index}\n${msToTimestamp(entry.startMs)} --> ${msToTimestamp(entry.endMs)}\n${entry.text}`;
            })
            .join("\n\n") + "\n"
    );
}

function msToVttTimestamp(ms: number): string {
    return msToTimestamp(ms).replace(",", ".");
}

export function serializeVttEntries(entries: SrtEntry[]): string {
    const cues = entries
        .map((entry) => `${msToVttTimestamp(entry.startMs)} --> ${msToVttTimestamp(entry.endMs)}\n${entry.text}`)
        .join("\n\n");
    return `WEBVTT\n\n${cues}${cues ? "\n" : ""}`;
}

export function formatSubtitleClock(ms: number): string {
    const safe = Math.max(0, Math.round(ms));
    const minutes = Math.floor(safe / 60_000);
    const seconds = Math.floor((safe % 60_000) / 1000);
    const millis = safe % 1000;
    return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

export function parseSubtitleClock(value: string, fallback: number): number {
    const match = value.trim().match(/^(\d{1,2}):(\d{2})(?:[.,](\d{1,3}))?$/);
    if (!match) return fallback;
    const minutes = Number(match[1]);
    const seconds = Number(match[2]);
    const millis = Number((match[3] || "0").padEnd(3, "0"));
    if ([minutes, seconds, millis].some((n) => Number.isNaN(n))) return fallback;
    return minutes * 60_000 + seconds * 1000 + millis;
}
