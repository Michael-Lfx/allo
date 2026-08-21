/**
 * Split local persist for canvas projects.
 *
 * The zustand main key only stores an id list plus title/updatedAt. Each
 * project's nodes/connections live at `infinite-canvas:project:${id}` so
 * editing the current canvas does not JSON.stringify every other project.
 */

export const CANVAS_STORE_KEY = "infinite-canvas:canvas_store";
export const CANVAS_PERSIST_FORMAT = 2 as const;

export function canvasProjectPersistKey(projectId: string) {
    return `infinite-canvas:project:${projectId}`;
}

type Awaitable<T> = T | Promise<T>;

export type CanvasPersistStorage = {
    getItem: (name: string) => Awaitable<string | null>;
    setItem: (name: string, value: string) => Awaitable<unknown>;
    removeItem: (name: string) => Awaitable<unknown>;
};

export type CanvasProjectIndexEntry = {
    id: string;
    title: string;
    createdAt: string;
    updatedAt: string;
    projectId?: string;
};

export type PersistedCanvasProjects<T extends CanvasProjectIndexEntry = CanvasProjectIndexEntry> = {
    state: { projects: T[] };
    version?: number;
};

type SplitCanvasPersistEnvelope = {
    persistFormat: typeof CANVAS_PERSIST_FORMAT;
    version?: number;
    state: { projectIndex: CanvasProjectIndexEntry[] };
};

function isSplitEnvelope(parsed: unknown): parsed is SplitCanvasPersistEnvelope {
    if (!parsed || typeof parsed !== "object") return false;
    const value = parsed as { persistFormat?: unknown; state?: { projectIndex?: unknown } };
    return value.persistFormat === CANVAS_PERSIST_FORMAT && Array.isArray(value.state?.projectIndex);
}

function isLegacyEnvelope<T extends CanvasProjectIndexEntry>(parsed: unknown): parsed is PersistedCanvasProjects<T> {
    if (!parsed || typeof parsed !== "object") return false;
    const value = parsed as { persistFormat?: unknown; state?: { projects?: unknown } };
    return value.persistFormat !== CANVAS_PERSIST_FORMAT && Array.isArray(value.state?.projects);
}

function projectIndexEntry(project: CanvasProjectIndexEntry): CanvasProjectIndexEntry {
    return {
        id: project.id,
        title: project.title,
        createdAt: project.createdAt,
        updatedAt: project.updatedAt,
        projectId: project.projectId,
    };
}

async function readProjectBlobs<T extends CanvasProjectIndexEntry>(storage: CanvasPersistStorage, index: CanvasProjectIndexEntry[]): Promise<T[]> {
    const projects: T[] = [];
    for (const entry of index) {
        const raw = await storage.getItem(canvasProjectPersistKey(entry.id));
        if (!raw) continue;
        try {
            projects.push(JSON.parse(raw) as T);
        } catch {
            // Skip a corrupt per-project blob rather than failing the whole gallery.
        }
    }
    return projects;
}

export async function writeSplitCanvasProjects<T extends CanvasProjectIndexEntry>(
    storage: CanvasPersistStorage,
    name: string,
    value: PersistedCanvasProjects<T>,
    previousProjects: T[] | null,
): Promise<void> {
    const nextProjects = value.state.projects;
    const previousById = new Map((previousProjects || []).map((project) => [project.id, project]));
    const nextIds = new Set(nextProjects.map((project) => project.id));

    for (const project of nextProjects) {
        if (previousById.get(project.id) === project) continue;
        await storage.setItem(canvasProjectPersistKey(project.id), JSON.stringify(project));
    }

    for (const previous of previousProjects || []) {
        if (!nextIds.has(previous.id)) await storage.removeItem(canvasProjectPersistKey(previous.id));
    }

    const envelope: SplitCanvasPersistEnvelope = {
        persistFormat: CANVAS_PERSIST_FORMAT,
        version: value.version,
        state: { projectIndex: nextProjects.map(projectIndexEntry) },
    };
    await storage.setItem(name, JSON.stringify(envelope));
}

export async function loadPersistedCanvasProjects<T extends CanvasProjectIndexEntry>(
    storage: CanvasPersistStorage,
    name: string,
): Promise<PersistedCanvasProjects<T> | null> {
    const raw = await storage.getItem(name);
    if (!raw) return null;

    let parsed: unknown;
    try {
        parsed = JSON.parse(raw);
    } catch {
        return null;
    }

    if (isSplitEnvelope(parsed)) {
        return { state: { projects: await readProjectBlobs<T>(storage, parsed.state.projectIndex) }, version: parsed.version };
    }

    if (isLegacyEnvelope<T>(parsed)) {
        await writeSplitCanvasProjects(storage, name, parsed, null);
        return { state: { projects: parsed.state.projects }, version: parsed.version };
    }

    return null;
}

export async function removePersistedCanvasProjects(
    storage: CanvasPersistStorage,
    name: string,
    projects: CanvasProjectIndexEntry[] | null,
): Promise<void> {
    await storage.removeItem(name);
    await Promise.all((projects || []).map((project) => storage.removeItem(canvasProjectPersistKey(project.id))));
}
