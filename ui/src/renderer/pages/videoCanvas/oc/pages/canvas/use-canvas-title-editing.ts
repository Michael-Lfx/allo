import { useCallback, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";

type CanvasTitleEditingInput = {
    currentProject: { title: string } | undefined;
    renameCurrentProject: ReturnType<typeof useCanvasProjectLifecycle>["renameCurrentProject"];
};

export function useCanvasTitleEditing(input: CanvasTitleEditingInput) {
    const { currentProject, renameCurrentProject } = input;
    const [titleEditing, setTitleEditing] = useState(false);
    const [titleDraft, setTitleDraft] = useState("");

    const startTitleEditing = useCallback(() => {
        setTitleDraft(currentProject?.title || canvasT("videoCanvas.chrome.untitled", "未命名画布"));
        setTitleEditing(true);
    }, [currentProject?.title]);

    const finishTitleEditing = useCallback(() => {
        const nextTitle = titleDraft.trim();
        if (nextTitle) renameCurrentProject(nextTitle);
        setTitleEditing(false);
    }, [renameCurrentProject, titleDraft]);
    return { titleEditing, setTitleEditing, titleDraft, setTitleDraft, startTitleEditing, finishTitleEditing };
}
