import { useEffect, useMemo, useState } from "react";
import { Button, Modal, Segmented } from "antd";
import { ImagePlus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { readImageMeta } from "@oc/lib/image-utils";
import { MAX_UPSCALE_LONG_EDGE, resolveUpscaleSize, type ImageUpscaleAlgorithm, type ImageUpscaleParams } from "@oc/lib/canvas/canvas-image-data";

export type CanvasImageUpscaleParams = ImageUpscaleParams;

function algorithms(): Array<{ value: ImageUpscaleAlgorithm; title: string; description: string }> {
    return [
        { value: "high", title: canvasT("videoCanvas.dialog.upscaleAlgoHigh", "高清插值"), description: canvasT("videoCanvas.dialog.upscaleAlgoHighDesc", "适合照片和细节图") },
        { value: "bilinear", title: canvasT("videoCanvas.dialog.upscaleAlgoBilinear", "双线性"), description: canvasT("videoCanvas.dialog.upscaleAlgoBilinearDesc", "平滑、速度快") },
        { value: "nearest", title: canvasT("videoCanvas.dialog.upscaleAlgoNearest", "最近邻"), description: canvasT("videoCanvas.dialog.upscaleAlgoNearestDesc", "适合像素风格") },
    ];
}

const targetOptions = [
    { label: "1K", value: 1024 },
    { label: "2K", value: 2048 },
    { label: "4K", value: MAX_UPSCALE_LONG_EDGE },
];

const defaultParams: CanvasImageUpscaleParams = {
    targetLongEdge: 2048,
    algorithm: "high",
};

export function CanvasNodeUpscaleDialog({ dataUrl, open, onClose, onConfirm }: { dataUrl: string; open: boolean; onClose: () => void; onConfirm: (params: CanvasImageUpscaleParams) => void }) {
    useTranslation();
    const [params, setParams] = useState<CanvasImageUpscaleParams>(defaultParams);
    const [image, setImage] = useState<{ width: number; height: number } | null>(null);
    const sourceLongEdge = image ? Math.max(image.width, image.height) : 0;
    const outputSize = useMemo(() => (image ? resolveUpscaleSize(image.width, image.height, params.targetLongEdge) : null), [image, params.targetLongEdge]);
    const canUpscale = Boolean(image && sourceLongEdge < params.targetLongEdge && params.targetLongEdge <= MAX_UPSCALE_LONG_EDGE);
    const reachedMax = Boolean(image && sourceLongEdge >= MAX_UPSCALE_LONG_EDGE);

    useEffect(() => {
        if (!open) return;
        setParams(defaultParams);
        setImage(null);
    }, [dataUrl, open]);

    useEffect(() => {
        if (!open) return;
        void readImageMeta(dataUrl).then(setImage);
    }, [dataUrl, open]);

    useEffect(() => {
        if (!image) return;
        const nextTarget = targetOptions.find((option) => sourceLongEdge < option.value)?.value || MAX_UPSCALE_LONG_EDGE;
        setParams((current) => ({ ...current, targetLongEdge: nextTarget }));
    }, [image, sourceLongEdge]);

    return (
        <Modal title={null} open={open && Boolean(dataUrl)} onCancel={onClose} footer={null} width={820} centered destroyOnHidden>
            <div className="space-y-5">
                <div>
                    <h2 className="text-xl font-semibold">{canvasT("videoCanvas.dialog.upscaleTitle", "图片放大")}</h2>
                </div>
                <div className="grid gap-6 md:grid-cols-[minmax(260px,1fr)_360px]">
                    <div className="rounded-xl border p-4">
                        <div className="grid min-h-[280px] place-items-center rounded-lg bg-black/5">
                            <img src={dataUrl} alt="" className="max-h-[320px] max-w-full rounded-lg object-contain shadow-xl" draggable={false} />
                        </div>
                        <div className="mt-3 flex items-center justify-between text-sm">
                            <span className="opacity-60">{canvasT("videoCanvas.dialog.upscaleSource", "源图")}</span>
                            <span className="font-semibold">{image ? `${image.width} x ${image.height} px` : canvasT("videoCanvas.dialog.upscaleReading", "读取中")}</span>
                        </div>
                    </div>
                    <div className="space-y-6 py-2">
                        <div className="space-y-2">
                            <div className="font-medium opacity-75">{canvasT("videoCanvas.dialog.upscaleTargetPx", "目标像素")}</div>
                            <Segmented
                                block
                                value={params.targetLongEdge}
                                options={targetOptions.map((option) => ({ label: `${option.label} · ${option.value}px`, value: option.value, disabled: Boolean(image && sourceLongEdge >= option.value) }))}
                                onChange={(value) => setParams((current) => ({ ...current, targetLongEdge: Number(value) }))}
                            />
                            {image && !canUpscale ? <div className="text-xs font-medium text-[#ef4444]">{reachedMax ? canvasT("videoCanvas.dialog.upscaleAt4k", "图片已达到 4K，无需放大") : canvasT("videoCanvas.dialog.upscaleAtTarget", "图片已达到当前目标像素，无需放大")}</div> : null}
                        </div>
                        <div className="space-y-2">
                            <div className="font-medium opacity-75">{canvasT("videoCanvas.dialog.upscaleAlgo", "放大算法")}</div>
                            <Segmented
                                block
                                value={params.algorithm}
                                options={algorithms().map((item) => ({
                                    value: item.value,
                                    label: (
                                        <span className="flex min-h-12 flex-col justify-center text-left leading-5">
                                            <span className="font-medium">{item.title}</span>
                                            <span className="text-xs opacity-55">{item.description}</span>
                                        </span>
                                    ),
                                }))}
                                onChange={(value) => setParams((current) => ({ ...current, algorithm: value as ImageUpscaleAlgorithm }))}
                            />
                        </div>
                        <div className="rounded-xl border px-4 py-3 text-sm">
                            <div className="flex items-center justify-between">
                                <span className="opacity-60">{canvasT("videoCanvas.dialog.upscaleOutput", "输出尺寸")}</span>
                                <span className="font-semibold">{outputSize ? `${outputSize.width} x ${outputSize.height} px` : canvasT("videoCanvas.dialog.upscaleUnknown", "未知")}</span>
                            </div>
                        </div>
                    </div>
                </div>
                <div className="flex justify-end">
                    <Button type="primary" size="large" icon={<ImagePlus className="size-4" />} disabled={!canUpscale} onClick={() => onConfirm(params)}>
                        {canvasT("videoCanvas.dialog.upscaleGenerate", "生成放大图")}
                    </Button>
                </div>
            </div>
        </Modal>
    );
}
