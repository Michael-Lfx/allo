/**
 * Canvas-facing skills API.
 *
 * 视频画布不读取 allo「设置 → 技能」通用技能库（那是给 Agent / 对话用的）。
 * 预设里的「技能」仅来自画布技能节点（见 canvas-skill-mentions / collectCanvasSkills）。
 */

export type SkillSort = 'popular' | 'new' | 'updated';
export type SkillScope = 'public' | 'mine' | 'created' | 'favorites';
export type SkillMediaType = 'image' | 'video';

export type SkillShowcaseMedia = {
  type: SkillMediaType;
  showcase_uri: string;
  showcase_url: string;
};

export type Skill = {
  skill_id: string;
  skill_name: string;
  description: string;
  instruction?: string;
  status: number;
  markdown_url: string;
  create_time: number;
  update_time: number;
  source: number;
  tag: string;
  sort_weight: number;
  is_private: boolean;
  like_count: number;
  is_like: boolean;
  owner_uid: string;
  effective_user: { name: string; avatar_url: string; uid: string };
  original_skill_id: string | null;
  showcase_media: SkillShowcaseMedia[];
  added_count: number;
  is_test: boolean;
  extra_info: string;
  is_added: boolean;
  is_owner: boolean;
};

export type SkillCategory = { value: string; label: string };

export type SkillList = {
  skills: Skill[];
  total_count: number;
  has_more: boolean;
  next_offset: number;
  page: number;
  page_size: number;
  categories: SkillCategory[];
};

export type ListSkillsInput = {
  page?: number;
  page_size?: number;
  scope?: SkillScope;
  sort?: SkillSort;
  search?: string;
  tag?: string;
};

export type SkillMutationInput = {
  skill_name: string;
  description: string;
  instruction: string;
  tag: string;
  is_private: boolean;
  markdown_url: string;
  showcase_media: SkillShowcaseMedia[];
  extra_info: string;
};

const emptySkillList = (input: ListSkillsInput = {}): SkillList => ({
  skills: [],
  total_count: 0,
  has_more: false,
  next_offset: 0,
  page: input.page || 1,
  page_size: input.page_size || 20,
  categories: [],
});

export function listSkills(input: ListSkillsInput = {}) {
  return Promise.resolve(emptySkillList(input));
}

export function getSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

/** 不接通用技能库；画布技能由节点侧 collectCanvasSkills 提供。 */
export function listAddedSkills() {
  return Promise.resolve({ skills: [] as Skill[] });
}

export function createSkill(_input: SkillMutationInput) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function updateSkill(_id: string, _input: SkillMutationInput) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function deleteSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function addSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function removeSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function likeSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}

export function unlikeSkill(_id: string) {
  return Promise.reject(new Error('Remote skill catalog is not available in allo canvas'));
}
