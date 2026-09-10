import { getCloudSkillDetail, getVerticalSkill, installCloudSkill, listCloudSkills } from "@renderer/pages/videoGeneration/api";
import type { VimaxCloudSkill } from "@renderer/pages/videoGeneration/types";

import type { CraftPlaybook } from "./types";

export function hubSkillToPlaybook(item: VimaxCloudSkill): CraftPlaybook {
    const qualifiedId = `hub:${item.id}`;
    return {
        kind: "playbook",
        id: `hub-${item.id}`,
        qualifiedId,
        title: { zh: item.displayName, en: item.displayName },
        job: { zh: item.description || item.howToUse || item.displayName, en: item.description || item.displayName },
        category: item.category || "drama",
        coverLookId: "cinematic",
        brief: item.description || item.displayName,
        origin: "hub",
    };
}

export async function listCanvasHubPlaybooks(keyword = ""): Promise<VimaxCloudSkill[]> {
    const result = await listCloudSkills({ page: 1, pageSize: 40, keyword: keyword || undefined, mode: "canvas", sort: "hot" });
    return result.list ?? [];
}

export async function installAndLoadHubPlaybook(id: number): Promise<{ playbook: CraftPlaybook; template: string; coverUrl?: string }> {
    await installCloudSkill(id);
    const detail = await getCloudSkillDetail(id);
    const playbook = hubSkillToPlaybook(detail);
    const template = detail.manifestText?.trim() || [detail.useScenario, itemHow(detail), detail.output].filter(Boolean).join("\n\n") || playbook.brief;
    return { playbook, template, coverUrl: detail.coverUrl || detail.previewUrl || undefined };
}

function itemHow(detail: VimaxCloudSkill) {
    return [detail.howToUse, detail.useScenario].filter(Boolean).join("\n");
}

export async function loadBuiltinPlaybookTemplate(qualifiedId: string, fallback: string): Promise<string> {
    try {
        const detail = await getVerticalSkill(qualifiedId);
        return detail.skill.playbook?.trim() || detail.manifest?.trim() || fallback;
    } catch {
        return fallback;
    }
}
