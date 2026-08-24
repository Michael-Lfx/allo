import { CanvasFreeformEmptyState, CanvasLinkedProjectEmptyState, CanvasShortDramaEmptyState } from "@oc/components/canvas/canvas-short-drama-entry";
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
};

export function CanvasProjectEmptyState(props: CanvasProjectEmptyStateProps) {
    const { shortDramaEnabled, currentProject, linkedProject, onUpload, onAddText, onAddScript, onAddChapter, onOpenAssets, onCreatePipeline, onOpenAgent } = props;
    return !shortDramaEnabled ? (
        <CanvasFreeformEmptyState onUpload={onUpload} onAddText={onAddText} />
    ) : currentProject?.projectId ? (
        <CanvasLinkedProjectEmptyState
            projectName={linkedProject?.project.name || currentProject.title}
            hasChapter={Boolean(linkedProject?.units.length)}
            onAddFirstChapter={() => {
                const first = linkedProject?.units.slice().sort((left, right) => left.position - right.position)[0];
                if (first) void onAddChapter({ id: first.id, projectId: currentProject.projectId!, title: first.title, position: first.position });
            }}
            onOpenAssets={onOpenAssets}
            onAddText={onAddText}
        />
    ) : (
        <CanvasShortDramaEmptyState onCreatePipeline={onCreatePipeline} onOpenAgent={onOpenAgent} onUpload={onUpload} onAddText={onAddText} onAddScript={onAddScript} />
    );
}
