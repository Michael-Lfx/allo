import { CanvasFreeformEmptyState, CanvasLinkedProjectEmptyState, CanvasShortDramaEmptyState } from "@oc/components/canvas/canvas-short-drama-entry";
import { resolveCanvasEmptyStateKind } from "@oc/lib/canvas/canvas-starter";
import type { CanvasProjectChapterPayload } from "@oc/components/canvas/canvas-project-sidebar";
import type { ProjectDetail } from "@oc/services/api/projects";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";

type CanvasProjectEmptyStateProps = {
    shortDramaEnabled: boolean;
    currentProject: ReturnType<typeof useCanvasProjectLifecycle>["currentProject"];
    linkedProject: ProjectDetail | undefined;
    onUpload: () => void;
    onAddText: () => void;
    onAddScript: () => void;
    onAddChapter: (chapter: CanvasProjectChapterPayload) => void | Promise<void>;
    onOpenAssets: () => void;
    onCreatePipeline: () => void;
    onOpenAgent: () => void;
    onStartFreeform: () => void;
};

export function CanvasProjectEmptyState(props: CanvasProjectEmptyStateProps) {
    const { shortDramaEnabled, currentProject, linkedProject, onUpload, onAddText, onAddScript, onAddChapter, onOpenAssets, onCreatePipeline, onOpenAgent, onStartFreeform } = props;
    const kind = resolveCanvasEmptyStateKind({
        nodeCount: 0,
        shortDramaEnabled,
        isProjectLinked: Boolean(currentProject?.projectId),
        starterMode: currentProject?.starterMode,
    });
    if (kind === "freeform") return <CanvasFreeformEmptyState onUpload={onUpload} onAddText={onAddText} />;
    if (kind === "linked") {
        return (
            <CanvasLinkedProjectEmptyState
                projectName={linkedProject?.project.name || currentProject?.title || ""}
                hasChapter={Boolean(linkedProject?.units.length)}
                onAddFirstChapter={() => {
                    const first = linkedProject?.units.slice().sort((left, right) => left.position - right.position)[0];
                    if (first && currentProject?.projectId) void onAddChapter({ id: first.id, projectId: currentProject.projectId, title: first.title, position: first.position });
                }}
                onOpenAssets={onOpenAssets}
                onAddText={onAddText}
            />
        );
    }
    return <CanvasShortDramaEmptyState onCreatePipeline={onCreatePipeline} onOpenAgent={onOpenAgent} onStartFreeform={onStartFreeform} onUpload={onUpload} onAddText={onAddText} onAddScript={onAddScript} />;
}
