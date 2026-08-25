import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import type { Skill } from "@oc/services/api/skills";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

const SKILL_REF_PATTERN = /@\[skill:([^\]]+)\]/g;

export function buildSkillMentionReferences(skills: Skill[]): CanvasResourceReference[] {
    return skills
        .filter((skill) => skill.is_added)
        .map((skill) => ({
            id: `skill:${skill.skill_id}`,
            nodeId: `skill:${skill.skill_id}`,
            kind: "skill" as const,
            label: skill.skill_name,
            title: skill.skill_name,
            text: skill.instruction || skill.description,
            active: true,
            skill,
        }));
}

/** 将画布技能节点转为与 OA「已加入技能」同形的 Skill，供预设 / @提及 / 生成展开共用。 */
export function skillFromCanvasNode(node: CanvasNodeData): Skill | null {
    if (node.type !== CanvasNodeType.Skill) return null;
    const snapshot = node.metadata?.skillSnapshot;
    if (!snapshot?.name?.trim() && !snapshot?.template?.trim()) return null;
    const skillId = snapshot.id || node.metadata?.skillId || node.id;
    const skillName = snapshot.name?.trim() || node.title?.trim() || "技能";
    return {
        skill_id: skillId,
        skill_name: skillName,
        description: snapshot.description || "",
        instruction: snapshot.template || "",
        status: 1,
        markdown_url: "",
        create_time: 0,
        update_time: 0,
        source: 0,
        tag: snapshot.category || "",
        sort_weight: 0,
        is_private: true,
        like_count: 0,
        is_like: false,
        owner_uid: "",
        effective_user: { name: "", avatar_url: "", uid: "" },
        original_skill_id: null,
        showcase_media: [],
        added_count: 0,
        is_test: false,
        extra_info: "",
        is_added: true,
        is_owner: true,
    };
}

/** 画布上的技能节点：视频模块内的技能来源（不读 allo 通用技能库）。 */
export function collectCanvasSkills(nodes: CanvasNodeData[]): Skill[] {
    const byId = new Map<string, Skill>();
    for (const node of nodes) {
        const skill = skillFromCanvasNode(node);
        if (!skill) continue;
        if (!byId.has(skill.skill_id)) byId.set(skill.skill_id, skill);
    }
    return [...byId.values()];
}

export function mergeSkillLists(...lists: Skill[][]): Skill[] {
    const byId = new Map<string, Skill>();
    for (const list of lists) {
        for (const skill of list) {
            if (!skill.is_added) continue;
            if (!byId.has(skill.skill_id)) byId.set(skill.skill_id, skill);
        }
    }
    return [...byId.values()];
}

export function expandSkillMentions(prompt: string, skills: Skill[]) {
    if (!prompt.trim()) return prompt;
    const activeSkills = skills.filter((skill) => skill.is_added);
    if (!activeSkills.length) return prompt;

    const byId = new Map(activeSkills.map((skill) => [skill.skill_id, skill]));
    let next = prompt.replace(SKILL_REF_PATTERN, (token, id) => {
        const skill = byId.get(id);
        return skill ? renderSkillPrompt(skill) : token;
    });

    activeSkills
        .slice()
        .sort((a, b) => b.skill_name.length - a.skill_name.length)
        .forEach((skill) => {
            next = replaceNaturalSkillMention(next, skill);
        });

    return next;
}

export function renderSkillPrompt(skill: Pick<Skill, "skill_name" | "description" | "instruction">) {
    return [
        `【技能：${skill.skill_name}】`,
        skill.description ? `用途：${skill.description}` : "",
        skill.instruction ? `执行指令：\n${skill.instruction}` : "",
        "请严格执行该技能，只输出结果，不要输出解释性套话。",
    ]
        .filter(Boolean)
        .join("\n\n");
}

function replaceNaturalSkillMention(value: string, skill: Skill) {
    const token = `@${skill.skill_name}`;
    let result = "";
    let index = 0;

    while (index < value.length) {
        const found = value.indexOf(token, index);
        if (found < 0) {
            result += value.slice(index);
            break;
        }
        const after = found + token.length;
        if (!hasMentionBoundary(value, after)) {
            result += value.slice(index, after);
            index = after;
            continue;
        }
        result += value.slice(index, found);
        result += renderSkillPrompt(skill);
        index = after;
    }

    return result;
}

function hasMentionBoundary(value: string, index: number) {
    const char = value[index];
    return !char || /\s|[,.!?;:，。！？；：、)\]}】）]/.test(char);
}
