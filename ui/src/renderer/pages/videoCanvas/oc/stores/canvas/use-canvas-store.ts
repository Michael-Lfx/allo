import { create } from "zustand";
import { persist, type PersistStorage, type StorageValue } from "zustand/middleware";

import { nanoid } from "nanoid";
import { canvasAppearanceForTheme, DEFAULT_CANVAS_COLOR_THEME, normalizeCanvasAppearance, readCanvasAppearanceDefault, type CanvasAppearance } from "@oc/lib/canvas/canvas-appearance";
import type { CanvasStarterMode } from "@oc/lib/canvas/canvas-starter";
import { localForageStorage } from "@oc/lib/localforage-storage";
import type { CanvasBackgroundMode } from "@oc/lib/canvas-theme";
import type { CanvasAssistantSession, CanvasConnection, CanvasNodeData, ViewportTransform } from "@oc/types/canvas";
import type { DirectorScene } from "@oc/types/director";
import type { TimelineProject } from "@oc/types/timeline";
import {
    CANVAS_STORE_KEY,
    loadPersistedCanvasProjects,
    removePersistedCanvasProjects,
    writeSplitCanvasProjects,
    type PersistedCanvasProjects,
} from "./canvas-store-persist";

export { CANVAS_STORE_KEY, canvasProjectPersistKey } from "./canvas-store-persist";

export type CanvasProject = {
    id: string;
    projectId?: string;
    title: string;
    createdAt: string;
    updatedAt: string;
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    chatSessions: CanvasAssistantSession[];
    activeChatId: string | null;
    starterMode?: CanvasStarterMode;
    appearance?: CanvasAppearance;
    backgroundMode: CanvasBackgroundMode;
    showImageInfo: boolean;
    viewport: ViewportTransform;
    directorScenes: DirectorScene[];
    /** 项目级时间线（client-doc）；无独立后端 API。 */
    timeline?: TimelineProject;
    /** Agent materialization sidecar from server doc.json */
    alloCreative?: Record<string, unknown>;
};

type CanvasStore = {
    hydrated: boolean;
    projects: CanvasProject[];
    createProject: (title?: string, projectId?: string) => string;
    importProject: (project: Partial<CanvasProject>) => string;
    openProject: (id: string) => CanvasProject | null;
    renameProject: (id: string, title: string) => void;
    deleteProjects: (ids: string[]) => void;
    replaceProjects: (projects: CanvasProject[]) => void;
    updateProject: (id: string, patch: Partial<Pick<CanvasProject, "projectId" | "nodes" | "connections" | "chatSessions" | "activeChatId" | "starterMode" | "appearance" | "backgroundMode" | "showImageInfo" | "viewport" | "directorScenes" | "timeline" | "alloCreative">>) => void;
};

const initialViewport: ViewportTransform = { x: 0, y: 0, k: 1 };
type PersistedCanvasState = Pick<CanvasStore, "projects">;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let lastPersistedProjects: CanvasProject[] | null = null;
let queuedPersistState: PersistedCanvasState | null = null;
let queuedPersistName = CANVAS_STORE_KEY;
let queuedPersistValue: PersistedCanvasProjects<CanvasProject> | null = null;
// 所有落盘经此链串行执行：单次 IndexedDB 写入可能超过防抖窗口，
// 不串行的话后到的 flush 会先落盘，慢的旧 payload 随后把 blob 和索引写回旧值。
let persistChain: Promise<void> = Promise.resolve();

const canvasStorage: PersistStorage<CanvasStore> = {
    getItem: async (name) => {
        const loaded = await loadPersistedCanvasProjects<CanvasProject>(localForageStorage, name);
        if (!loaded) return null;
        queuedPersistState = loaded.state;
        lastPersistedProjects = loaded.state.projects;
        return loaded as StorageValue<CanvasStore>;
    },
    setItem: (name, value) => {
        const nextState = value.state as PersistedCanvasState;
        if (queuedPersistState && queuedPersistState.projects === nextState.projects) return;
        queuedPersistState = nextState;
        queuedPersistName = name;
        queuedPersistValue = { state: nextState, version: value.version };
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            saveTimer = null;
            void enqueuePersistFlush();
        }, 400);
    },
    removeItem: (name) => removePersistedCanvasProjects(localForageStorage, name, lastPersistedProjects),
};

function runQueuedPersistFlush() {
    const payload = queuedPersistValue;
    queuedPersistValue = null;
    if (!payload) return;
    return writeSplitCanvasProjects(localForageStorage, queuedPersistName, payload, lastPersistedProjects).then(() => {
        lastPersistedProjects = payload.state.projects;
    });
}

function enqueuePersistFlush(): Promise<void> {
    const run = persistChain.then(runQueuedPersistFlush, runQueuedPersistFlush);
    persistChain = run.then(
        () => undefined,
        () => undefined,
    );
    return run;
}

export async function flushCanvasStorePersistence() {
    // 强刷语义：等所有在途写完成后把队列排空再返回；
    // 排空期间若又有新写入入队则继续，保证返回时磁盘已是最新状态。
    for (;;) {
        if (saveTimer) {
            clearTimeout(saveTimer);
            saveTimer = null;
        }
        await enqueuePersistFlush();
        if (!queuedPersistValue && !saveTimer) return;
    }
}

export const useCanvasStore = create<CanvasStore>()(
    persist(
        (set, get) => ({
            hydrated: false,
            projects: [],
            createProject: (title = "未命名画布", projectId) => {
                const now = new Date().toISOString();
                const id = nanoid();
                const appearanceDefault = readCanvasAppearanceDefault();
                const project: CanvasProject = {
                    id,
                    projectId,
                    title,
                    createdAt: now,
                    updatedAt: now,
                    nodes: [],
                    connections: [],
                    chatSessions: [],
                    activeChatId: null,
                    appearance: appearanceDefault?.appearance ?? canvasAppearanceForTheme(DEFAULT_CANVAS_COLOR_THEME),
                    backgroundMode: appearanceDefault?.backgroundMode || "lines",
                    showImageInfo: false,
                    viewport: initialViewport,
                    directorScenes: [],
                };
                set((state) => ({ projects: [project, ...state.projects] }));
                return id;
            },
            importProject: (source) => {
                const now = new Date().toISOString();
                const appearanceDefault = readCanvasAppearanceDefault();
                const project: CanvasProject = {
                    id: nanoid(),
                    projectId: source.projectId,
                    title: source.title || "导入画布",
                    createdAt: source.createdAt || now,
                    updatedAt: now,
                    nodes: source.nodes || [],
                    connections: source.connections || [],
                    chatSessions: source.chatSessions || [],
                    activeChatId: source.activeChatId || null,
                    starterMode: source.starterMode,
                    appearance: source.appearance ? normalizeCanvasAppearance(source.appearance, DEFAULT_CANVAS_COLOR_THEME) : appearanceDefault?.appearance ?? canvasAppearanceForTheme(DEFAULT_CANVAS_COLOR_THEME),
                    backgroundMode: source.backgroundMode || appearanceDefault?.backgroundMode || "lines",
                    showImageInfo: source.showImageInfo || false,
                    viewport: source.viewport || initialViewport,
                    directorScenes: source.directorScenes || [],
                };
                set((state) => ({ projects: [project, ...state.projects] }));
                return project.id;
            },
            openProject: (id) => {
                return get().projects.find((item) => item.id === id) || null;
            },
            renameProject: (id, title) =>
                set((state) => ({
                    projects: state.projects.map((project) => (project.id === id ? { ...project, title: title.trim() || project.title, updatedAt: new Date().toISOString() } : project)),
                })),
            deleteProjects: (ids) =>
                set((state) => {
                    const projects = state.projects.filter((project) => !ids.includes(project.id));
                    return { projects };
                }),
            replaceProjects: (projects) => set({ projects }),
            updateProject: (id, patch) =>
                set((state) => ({
                    projects: state.projects.map((project) => (project.id === id ? { ...project, ...patch, updatedAt: new Date().toISOString() } : project)),
                })),
        }),
        {
            name: CANVAS_STORE_KEY,
            storage: canvasStorage,
            partialize: (state) =>
                ({
                    projects: state.projects,
                }) as StorageValue<CanvasStore>["state"],
            onRehydrateStorage: () => () => {
                useCanvasStore.setState({ hydrated: true });
            },
        },
    ),
);
