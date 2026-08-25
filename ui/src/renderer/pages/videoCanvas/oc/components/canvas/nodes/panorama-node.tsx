import { useEffect, useRef, useState } from "react";
import { Globe, X } from "lucide-react";

import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

type PanoramaNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
    reduceMediaEffects?: boolean;
};

/**
 * 360° 全景查看器：取上游图片，拖拽环视。
 * three 按需动态加载；性能模式下仅静态预览。
 */
export function PanoramaNodeContent({ node, theme, reduceMediaEffects }: PanoramaNodeContentProps) {
    const upstream = useUpstreamNodes(node.id);
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "image");
    const url = node.metadata?.content || inherited?.metadata?.content || "";
    const [active, setActive] = useState(false);
    const [failed, setFailed] = useState(false);

    useEffect(() => {
        setActive(false);
        setFailed(false);
    }, [url]);

    if (!url) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <Globe className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>{canvasT("videoCanvas.extension.panoramaEmpty", "连接一张 360° 等距柱状全景图")}</span>
            </div>
        );
    }

    if (!active) {
        return (
            <div className="relative h-full w-full overflow-hidden" style={{ background: theme.node.fill }}>
                <img src={url} alt={node.title || canvasT("videoCanvas.node.panorama", "全景")} className="h-full w-full object-cover opacity-80" draggable={false} />
                <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 px-3 py-2" style={{ background: "linear-gradient(to top, rgba(0,0,0,.85), rgba(0,0,0,.35))", boxShadow: "0 -1px 0 rgba(0,0,0,.25)" }}>
                    <span className="min-w-0 truncate text-white" style={{ fontSize: "var(--fs-label)", textShadow: "0 1px 3px rgba(0,0,0,.85)" }}>
                        {failed
                            ? canvasT("videoCanvas.extension.panoramaFailed", "全景加载失败（图片可能不允许跨域读取）")
                            : reduceMediaEffects
                              ? canvasT("videoCanvas.extension.panoramaPerf", "性能模式下仅显示预览")
                              : canvasT("videoCanvas.extension.panoramaHint", "360° 全景")}
                    </span>
                    {reduceMediaEffects ? null : (
                        <button
                            type="button"
                            className="shrink-0 rounded-[var(--r-md)] px-2 py-1 font-medium text-white outline-none transition-colors"
                            style={{ fontSize: "var(--fs-label)", background: "rgba(79,110,232,.92)", boxShadow: "0 1px 4px rgba(0,0,0,.35)" }}
                            onMouseDown={(event) => event.stopPropagation()}
                            onClick={() => {
                                setFailed(false);
                                setActive(true);
                            }}
                        >
                            {canvasT("videoCanvas.extension.panoramaEnter", "进入环视")}
                        </button>
                    )}
                </div>
            </div>
        );
    }

    return (
        <div className="relative h-full w-full overflow-hidden" style={{ background: "#000" }}>
            <PanoramaViewport
                url={url}
                onError={() => {
                    setFailed(true);
                    setActive(false);
                }}
            />
            <button
                type="button"
                aria-label={canvasT("videoCanvas.extension.panoramaExit", "退出环视")}
                className="absolute right-2 top-2 grid size-6 place-items-center rounded-full text-white outline-none transition-colors"
                style={{ background: "rgba(79,110,232,.85)", boxShadow: "0 1px 4px rgba(0,0,0,.4)" }}
                onMouseDown={(event) => event.stopPropagation()}
                onClick={() => setActive(false)}
            >
                <X className="size-3.5" />
            </button>
        </div>
    );
}

function PanoramaViewport({ url, onError }: { url: string; onError: () => void }) {
    const hostRef = useRef<HTMLDivElement | null>(null);
    const onErrorRef = useRef(onError);
    onErrorRef.current = onError;

    useEffect(() => {
        const host = hostRef.current;
        if (!host) return;
        let disposed = false;
        let frame = 0;
        const teardown: Array<() => void> = [];

        void (async () => {
            const THREE = await import("three");
            if (disposed) return;

            const renderer = new THREE.WebGLRenderer({ antialias: true });
            renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
            const scene = new THREE.Scene();
            const camera = new THREE.PerspectiveCamera(75, 1, 0.1, 1100);

            const loader = new THREE.TextureLoader();
            loader.setCrossOrigin("anonymous");
            const texture = await new Promise<InstanceType<typeof THREE.Texture> | null>((resolve) => {
                loader.load(url, resolve, undefined, () => resolve(null));
            });
            if (disposed) {
                renderer.dispose();
                return;
            }
            if (!texture) {
                renderer.dispose();
                onErrorRef.current();
                return;
            }

            texture.colorSpace = THREE.SRGBColorSpace;
            const geometry = new THREE.SphereGeometry(500, 60, 40);
            const material = new THREE.MeshBasicMaterial({ map: texture, side: THREE.BackSide });
            scene.add(new THREE.Mesh(geometry, material));
            host.appendChild(renderer.domElement);
            renderer.domElement.style.cssText = "width:100%;height:100%;display:block;cursor:grab";

            let lon = 0;
            let lat = 0;
            let dragging = false;
            let lastX = 0;
            let lastY = 0;

            const onPointerDown = (event: PointerEvent) => {
                dragging = true;
                lastX = event.clientX;
                lastY = event.clientY;
                renderer.domElement.setPointerCapture(event.pointerId);
                event.stopPropagation();
            };
            const onPointerMove = (event: PointerEvent) => {
                if (!dragging) return;
                lon -= (event.clientX - lastX) * 0.18;
                lat += (event.clientY - lastY) * 0.18;
                lat = Math.max(-85, Math.min(85, lat));
                lastX = event.clientX;
                lastY = event.clientY;
            };
            const onPointerUp = (event: PointerEvent) => {
                dragging = false;
                if (renderer.domElement.hasPointerCapture(event.pointerId)) renderer.domElement.releasePointerCapture(event.pointerId);
            };
            renderer.domElement.addEventListener("pointerdown", onPointerDown);
            renderer.domElement.addEventListener("pointermove", onPointerMove);
            renderer.domElement.addEventListener("pointerup", onPointerUp);
            renderer.domElement.addEventListener("pointercancel", onPointerUp);

            const resize = () => {
                const { clientWidth, clientHeight } = host;
                if (!clientWidth || !clientHeight) return;
                renderer.setSize(clientWidth, clientHeight, false);
                camera.aspect = clientWidth / clientHeight;
                camera.updateProjectionMatrix();
            };
            const observer = new ResizeObserver(resize);
            observer.observe(host);
            resize();

            const tick = () => {
                const phi = THREE.MathUtils.degToRad(90 - lat);
                const theta = THREE.MathUtils.degToRad(lon);
                camera.lookAt(500 * Math.sin(phi) * Math.cos(theta), 500 * Math.cos(phi), 500 * Math.sin(phi) * Math.sin(theta));
                renderer.render(scene, camera);
                frame = requestAnimationFrame(tick);
            };
            tick();

            teardown.push(() => {
                cancelAnimationFrame(frame);
                observer.disconnect();
                renderer.domElement.removeEventListener("pointerdown", onPointerDown);
                renderer.domElement.removeEventListener("pointermove", onPointerMove);
                renderer.domElement.removeEventListener("pointerup", onPointerUp);
                renderer.domElement.removeEventListener("pointercancel", onPointerUp);
                renderer.domElement.remove();
                geometry.dispose();
                material.dispose();
                texture.dispose();
                renderer.forceContextLoss();
                renderer.dispose();
            });
        })();

        return () => {
            disposed = true;
            teardown.forEach((fn) => fn());
        };
    }, [url]);

    return <div ref={hostRef} className="h-full w-full" data-canvas-no-zoom onMouseDown={(event) => event.stopPropagation()} />;
}
