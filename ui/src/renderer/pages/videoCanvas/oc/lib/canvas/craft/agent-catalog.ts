import { collectCanvasSkills } from "@oc/lib/canvas/canvas-skill-mentions";
import type { CanvasNodeData } from "@oc/types/canvas";

import { BUILTIN_PLAYBOOKS, PLAYBOOK_BY_QUALIFIED } from "./playbooks";

export type AgentPlaybookSummary = {
    skillId: string;
    name: string;
    description: string;
    tag: string;
};

export type AgentPlaybookDetail = AgentPlaybookSummary & {
    instruction: string;
    version: number;
};

export function listAgentPlaybooks(nodes: CanvasNodeData[]): AgentPlaybookSummary[] {
    const byId = new Map<string, AgentPlaybookSummary>();
    for (const item of BUILTIN_PLAYBOOKS) {
        byId.set(item.qualifiedId, { skillId: item.qualifiedId, name: item.title.zh, description: item.job.zh, tag: item.category });
    }
    for (const skill of collectCanvasSkills(nodes)) {
        byId.set(skill.skill_id, { skillId: skill.skill_id, name: skill.skill_name, description: skill.description, tag: skill.tag });
    }
    return [...byId.values()];
}

export function getAgentPlaybook(nodes: CanvasNodeData[], skillId = "", name = ""): AgentPlaybookDetail | null {
    const needle = name.trim().toLocaleLowerCase();
    const placed = collectCanvasSkills(nodes).find((item) => item.skill_id === skillId || (needle && item.skill_name.toLocaleLowerCase() === needle));
    if (placed) {
        return { skillId: placed.skill_id, name: placed.skill_name, description: placed.description, tag: placed.tag, instruction: placed.instruction || placed.description, version: placed.update_time || 1 };
    }
    const playbook = PLAYBOOK_BY_QUALIFIED.get(skillId)
        || PLAYBOOK_BY_QUALIFIED.get(`builtin:${skillId}`)
        || BUILTIN_PLAYBOOKS.find((item) => item.id === skillId || item.title.zh === name.trim() || (needle && item.title.en.toLowerCase() === needle));
    if (!playbook) return null;
    return { skillId: playbook.qualifiedId, name: playbook.title.zh, description: playbook.job.zh, tag: playbook.category, instruction: playbook.brief, version: 1 };
}
