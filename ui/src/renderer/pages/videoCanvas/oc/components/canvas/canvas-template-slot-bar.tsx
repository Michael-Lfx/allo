import { useState } from "react";

import type { CanvasTheme } from "@oc/lib/canvas-theme";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { rewriteCompiledPrompt, rewriteSlotsFromInstruction } from "@oc/lib/canvas/generation-template/compile";
import type { CanvasNodeMetadata } from "@oc/types/canvas";

export function CanvasTemplateSlotBar({
    theme,
    metadata,
    model,
    onChange,
}: {
    theme: CanvasTheme;
    metadata?: CanvasNodeMetadata;
    model?: string;
    onChange: (patch: Partial<CanvasNodeMetadata>) => void;
}) {
    const stamp = metadata?.appliedTemplate;
    const slots = (stamp?.slots || []).filter((slot) => slot.kind !== "asset");
    const [instruction, setInstruction] = useState("");
    if (!stamp || !slots.length) return null;
    const values = { ...(metadata?.promptTemplateVariables || {}) };
    const rewrite = (nextValues: Record<string, string>) => {
        const prompt = rewriteCompiledPrompt(stamp, model || metadata?.model, nextValues);
        onChange({ promptTemplateVariables: nextValues, composerContent: prompt, prompt });
    };
    const applyInstruction = () => {
        const text = instruction.trim();
        if (!text) return;
        rewrite(rewriteSlotsFromInstruction(stamp.slots, values, text));
        setInstruction("");
    };
    return (
        <div className="flex flex-col gap-1.5 px-0.5 pb-1">
            <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-[10px] font-medium" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.templateSlots", "槽位")}</span>
                {slots.map((slot) => (
                    slot.kind === "enum" && slot.options?.length ? (
                        <select
                            key={slot.id}
                            className="h-6 max-w-36 rounded-md border bg-transparent px-1 text-[11px]"
                            style={{ borderColor: theme.node.stroke, color: theme.node.text }}
                            value={values[slot.id] || slot.default || ""}
                            aria-label={slot.label || slot.id}
                            onChange={(event) => rewrite({ ...values, [slot.id]: event.target.value })}
                        >
                            {slot.options.map((option) => (
                                <option key={option} value={option}>{option}</option>
                            ))}
                        </select>
                    ) : (
                        <input
                            key={slot.id}
                            className="h-6 max-w-40 rounded-md border bg-transparent px-1.5 text-[11px]"
                            style={{ borderColor: theme.node.stroke, color: theme.node.text }}
                            value={values[slot.id] || ""}
                            placeholder={slot.label || slot.id}
                            aria-label={slot.label || slot.id}
                            onChange={(event) => rewrite({ ...values, [slot.id]: event.target.value })}
                        />
                    )
                ))}
            </div>
            <div className="flex items-center gap-1.5">
                <input
                    className="h-7 min-w-0 flex-1 rounded-md border bg-transparent px-2 text-[11px]"
                    style={{ borderColor: theme.node.stroke, color: theme.node.text }}
                    value={instruction}
                    placeholder={canvasT("videoCanvas.craft.templateRewritePlaceholder", "用一句话改这个模板，例如：改成海边散步")}
                    aria-label={canvasT("videoCanvas.craft.templateRewritePlaceholder", "用一句话改这个模板，例如：改成海边散步")}
                    onChange={(event) => setInstruction(event.target.value)}
                    onKeyDown={(event) => {
                        if (event.key === "Enter") {
                            event.preventDefault();
                            applyInstruction();
                        }
                    }}
                />
                <button
                    type="button"
                    className="h-7 shrink-0 rounded-md px-2 text-[11px] font-medium"
                    style={{ color: theme.accent.primary, background: theme.toolbar.itemHover }}
                    onClick={applyInstruction}
                >
                    {canvasT("videoCanvas.craft.templateRewriteApply", "改写")}
                </button>
            </div>
        </div>
    );
}
