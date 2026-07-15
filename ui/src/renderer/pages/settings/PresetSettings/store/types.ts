/** 商店预设模板 — 安装时通过 ipcBridge.presets.create 写入本地 */
export interface StorePresetTemplate {
  id: string;
  name: string;
  name_i18n: Record<string, string>;
  description: string;
  description_i18n: Record<string, string>;
  avatar: string;
  instructions: string;
  included_skills: string[];
  audience_tags: string[];
  scenario_tags: string[];
  category: string;
  installCount: number;
}

/** 商店分类 */
export interface StoreCategory {
  key: string;
  label: string;
  label_i18n: Record<string, string>;
}

/** 静态商店数据 */
export interface StoreData {
  version: number;
  categories: StoreCategory[];
  templates: StorePresetTemplate[];
}
