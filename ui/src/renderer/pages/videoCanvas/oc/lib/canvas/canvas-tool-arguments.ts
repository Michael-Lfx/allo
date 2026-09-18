/**
 * Models and OpenAI-compatible proxies do not always emit tool arguments as a
 * JSON object string. Some return an already-parsed object, markdown fences,
 * trailing commas, or a truncated payload. The canvas agent loop must recover
 * a record before executing canvas_* tools.
 */

export function encodeToolArguments(value: unknown): string {
    if (typeof value === "string") return value;
    if (value == null) return "{}";
    try {
        return JSON.stringify(value);
    } catch {
        return "{}";
    }
}

export function parseToolArguments(value: unknown): Record<string, unknown> {
    const record = coerceJsonRecord(value, 0);
    if (!record) throw new Error("工具参数不是合法 JSON 对象");
    return record;
}

function coerceJsonRecord(value: unknown, depth: number): Record<string, unknown> | null {
    if (depth > 3) return null;
    if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
    if (typeof value !== "string") return value == null || value === "" ? {} : null;

    const text = unwrapToolArgumentText(value);
    if (!text) return {};

    const candidates = [text];
    const objectStart = text.indexOf("{");
    if (objectStart > 0 && !text.startsWith('"')) candidates.push(text.slice(objectStart));

    for (const candidate of candidates) {
        const parsed = parseJsonValue(candidate) ?? parseJsonValue(repairJsonText(candidate));
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) return parsed as Record<string, unknown>;
        if (typeof parsed === "string") {
            const nested = coerceJsonRecord(parsed, depth + 1);
            if (nested) return nested;
        }
    }
    return null;
}

function unwrapToolArgumentText(value: string) {
    let text = value.trim().replace(/^\uFEFF/, "");
    const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
    if (fenced?.[1]) text = fenced[1].trim();
    text = text.replace(/^(?:arguments|args|parameters|input)\s*[:=]\s*/i, "").trim();
    return text;
}

function parseJsonValue(text: string): unknown {
    try {
        return JSON.parse(text);
    } catch {
        return undefined;
    }
}

function repairJsonText(source: string) {
    let text = source.replace(/,\s*([}\]])/g, "$1").replace(/\bundefined\b/g, "null");
    if (isInsideString(text)) text += '"';
    text = text.replace(/,\s*([}\]])/g, "$1");
    const stack: Array<"{" | "["> = [];
    let inString = false;
    let escaped = false;
    for (let index = 0; index < text.length; index += 1) {
        const char = text[index];
        if (inString) {
            if (escaped) escaped = false;
            else if (char === "\\") escaped = true;
            else if (char === '"') inString = false;
            continue;
        }
        if (char === '"') {
            inString = true;
            continue;
        }
        if (char === "{" || char === "[") stack.push(char);
        if (char === "}" || char === "]") stack.pop();
    }
    while (stack.length) {
        text += stack.pop() === "{" ? "}" : "]";
    }
    return text;
}

function isInsideString(text: string) {
    let inString = false;
    let escaped = false;
    for (let index = 0; index < text.length; index += 1) {
        const char = text[index];
        if (!inString) {
            if (char === '"') inString = true;
            continue;
        }
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === '"') inString = false;
    }
    return inString;
}
