import { Button, Modal } from "antd";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { useAssetStore } from "@oc/stores/use-asset-store";
import { useCanvasStore } from "@oc/stores/canvas/use-canvas-store";
import { useCanvasUiStore } from "@oc/stores/canvas/use-canvas-ui-store";

export function CanvasDeleteProjectsDialog() {
    useTranslation();
    const ids = useCanvasUiStore((state) => state.deleteProjectIds);
    const setDeleteIds = useCanvasUiStore((state) => state.setDeleteProjectIds);
    const removeSelectedIds = useCanvasUiStore((state) => state.removeSelectedProjectIds);
    const deleteProjects = useCanvasStore((state) => state.deleteProjects);
    const cleanupImages = useAssetStore((state) => state.cleanupImages);
    const confirm = () => {
        deleteProjects(ids);
        cleanupImages();
        removeSelectedIds(ids);
        setDeleteIds([]);
    };

    return (
        <Modal
            title={canvasT("videoCanvas.dialog.deleteCanvasesTitle", "删除画布？")}
            open={ids.length > 0}
            centered
            onCancel={() => setDeleteIds([])}
            footer={
                <>
                    <Button onClick={() => setDeleteIds([])}>{canvasT("videoCanvas.dialog.cancel", "取消")}</Button>
                    <Button danger type="primary" onClick={confirm}>
                        {canvasT("videoCanvas.dialog.delete", "删除")}
                    </Button>
                </>
            }
        >
            <p className="text-sm text-stone-500">{canvasT("videoCanvas.dialog.deleteCanvasesBody", "将删除 {{count}} 个画布，里面的节点和连线也会一起移除。", { count: ids.length })}</p>
        </Modal>
    );
}
