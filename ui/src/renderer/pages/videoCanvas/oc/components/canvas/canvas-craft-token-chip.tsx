import { X } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { craftCover, craftStillUrl, craftText, PLAYBOOK_BY_QUALIFIED, RECIPE_BY_ID } from "@oc/lib/canvas/craft/catalog";
import { recipeToken } from "@oc/lib/canvas/craft/tokens";
import { skillToken } from "@oc/lib/canvas/canvas-skill-mentions";

import { CanvasStyleCoverSwatch } from "./canvas-style-cover";

export type CraftAttachmentKind = "recipe" | "skill";

type Theme = (typeof canvasThemes)[keyof typeof canvasThemes];

export type CraftAttachmentChipModel = {
    kind: CraftAttachmentKind;
    id: string;
    title: string;
    coverId: string;
    coverUrl?: string;
};

export function recipeAttachmentChip(id: string): CraftAttachmentChipModel {
    const recipe = RECIPE_BY_ID.get(id);
    return {
        kind: "recipe",
        id,
        title: recipe ? craftText(recipe.title) : id,
        coverId: recipe?.coverLookId || id,
    };
}

export function skillAttachmentChip(id: string, title?: string, coverUrl?: string): CraftAttachmentChipModel {
    const playbook = PLAYBOOK_BY_QUALIFIED.get(id);
    return {
        kind: "skill",
        id,
        title: title || (playbook ? craftText(playbook.title) : id),
        coverId: playbook?.coverLookId || "cinematic",
        coverUrl,
    };
}

export function CanvasCraftTokenChip({
    chip,
    theme,
    onRemove,
}: {
    chip: CraftAttachmentChipModel;
    theme: Theme;
    onRemove?: () => void;
}) {
    const cover = chip.coverUrl ? { ...craftCover(chip.coverId), image: chip.coverUrl } : craftCover(chip.coverId);
    const removeLabel = canvasT("videoCanvas.preset.removeAttachment", "移除 {{title}}", { title: chip.title });
    return (
        <span
            className="canvas-craft-token-chip"
            style={{ borderColor: theme.node.stroke, background: theme.toolbar.panel, color: theme.node.text }}
            title={chip.title}
        >
            <CanvasStyleCoverSwatch cover={cover} className="size-7 shrink-0 rounded-[6px]" alt="" />
            <span className="min-w-0 flex-1 truncate text-[12px] font-medium leading-none tracking-[-0.01em]">{chip.title}</span>
            {onRemove ? (
                <button type="button" className="canvas-craft-token-chip-remove" onClick={onRemove} aria-label={removeLabel} title={removeLabel}>
                    <X className="size-3" strokeWidth={2.25} />
                </button>
            ) : null}
        </span>
    );
}

export function createCraftAttachmentChipElement(chip: CraftAttachmentChipModel, theme: Theme): HTMLSpanElement {
    const wrapper = document.createElement("span");
    wrapper.contentEditable = "false";
    wrapper.className = "canvas-craft-token-chip canvas-craft-token-chip--inline";
    wrapper.title = chip.title;
    if (chip.kind === "recipe") wrapper.dataset.recipeId = chip.id;
    else wrapper.dataset.skillId = chip.id;
    wrapper.dataset.mentionToken = chip.kind === "recipe" ? recipeToken(chip.id) : skillToken(chip.id);
    Object.assign(wrapper.style, {
        borderColor: theme.node.stroke,
        background: theme.toolbar.panel,
        color: theme.node.text,
        verticalAlign: "middle",
    } as Partial<CSSStyleDeclaration>);

    const still = document.createElement("img");
    still.src = chip.coverUrl || craftStillUrl(chip.coverId);
    still.alt = "";
    still.draggable = false;
    still.style.cssText = "width:28px;height:28px;flex-shrink:0;border-radius:6px;object-fit:cover;";
    still.addEventListener("error", () => {
        still.remove();
    });

    const label = document.createElement("span");
    label.className = "min-w-0 flex-1 truncate text-[12px] font-medium leading-none";
    label.textContent = chip.title;

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "canvas-craft-token-chip-remove";
    remove.setAttribute("aria-label", canvasT("videoCanvas.preset.removeAttachment", "移除 {{title}}", { title: chip.title }));
    remove.textContent = "×";
    remove.addEventListener("pointerdown", (event) => {
        event.preventDefault();
        event.stopPropagation();
    });
    remove.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        const editor = wrapper.closest("[contenteditable='true']");
        wrapper.remove();
        editor?.dispatchEvent(new Event("input", { bubbles: true }));
    });

    wrapper.append(still, label, remove);
    return wrapper;
}
