-- 课时多引用（t4 评审补齐：t3 把"课时 source 多引用"划给 t4，t4 只做了
-- `kb_id` 归属）：`learning_lessons` 增加来源路径列表。
--
-- 列口径仿 `learning_courses.source_kb_ids`（迁移 073）与
-- `cs_agents.knowledge_base_ids`（迁移 019）：JSON 数组，CHECK 只放行数组。
-- **首个元素 = 主来源**，且与既有的三列（`source_path`/`source_start`/
-- `source_end`，迁移 015）同值——三列保留不动（既有读侧与索引不退役），
-- 本列是它的伴生扩展：老行由三列回填为单元素数组（无来源的 NULL 行回填为
-- 空数组，读侧回退三列）。
--
-- 元素形状与 `SourceSpan` 序列化同构（`{"path":…,"start":…,"end":…}`）；
-- 库归属（`kb_id`）不落库——读侧按采样命名空间实时解析，与课时单列
-- 路径的读侧口径逐字一致（双读与已删库过滤见 source_kb_ids 的 ADR）。
ALTER TABLE learning_lessons ADD COLUMN source_paths_json TEXT NOT NULL DEFAULT '[]' CHECK (
    json_valid(source_paths_json) AND json_type(source_paths_json) = 'array'
);

UPDATE learning_lessons
SET source_paths_json = json_array(
        json_object('path', source_path, 'start', source_start, 'end', source_end)
    )
WHERE source_path IS NOT NULL;
