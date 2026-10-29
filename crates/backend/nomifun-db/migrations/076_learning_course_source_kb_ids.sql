-- 多知识库（t4）：`learning_courses` 增加来源库列表。
--
-- 列口径仿 `cs_agents.knowledge_base_ids`（迁移 019 先例）：JSON 数组，
-- CHECK 只放行数组。**首个元素 = 主来源**，且与既有的单列
-- `source_kb_id`（迁移 015）同值——单列保留不动（既有读侧、SetNull 引用
-- 条目与索引都不退役），本列是它的伴生扩展：老行由单列回填为单元素数组
-- （描述流/已删库的 NULL 行回填为空数组，读侧对已删库 id 过滤）。
--
-- 不做物理外键（v3 规则）：引用完整性由应用层读侧过滤 + 单列既有契约承担；
-- 库里残留的已删库 id 由读侧剔除，不外泄悬空 id（t4 裁决）。
ALTER TABLE learning_courses ADD COLUMN source_kb_ids TEXT NOT NULL DEFAULT '[]' CHECK (
    json_valid(source_kb_ids) AND json_type(source_kb_ids) = 'array'
);

UPDATE learning_courses
SET source_kb_ids = json_array(source_kb_id)
WHERE source_kb_id IS NOT NULL;
