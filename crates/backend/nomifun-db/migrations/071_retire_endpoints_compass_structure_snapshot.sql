-- 学习模块吸收 learnhub（ADR-0010）：终点锚体系与罗盘整体退役 + 结构快照落点。
--
-- 破坏性迁移（066 先例）：建课从「目标 → AI 提议终点 → 多终点选择 → 生长」
-- 改为「目标 → 一次生成知识结构（结构 + 前置地形）→ 生长」，结构仅作参考
-- 地图，学习顺序权威归教练 + 已教概念账：
-- - learning_course_endpoints 整表 DROP：终点不再是交互与判定面的一等公民
--   （提议端点、完成裁决、停摆臂第二臂、终点保护条款随之退役）；方向语义
--   由结构产物的读侧投影承担（路线图/覆盖徽章，永不落盘）。
-- - 每条终点兼作一行零正文标记课时（lesson_id UNIQUE），先清其进度行再删
--   课时行，随后整表 DROP（两个专用索引随表消失）。
-- - learning_courses.compass_md / compass_updated_at DROP：罗盘的分节前提
--   （终点）消失，逐终点剩余路线摘要连同重画管道整体退役。
-- - learning_courses.structure_json ADD：知识结构 + 前置地形两产物的单字段
--   JSON 快照（ADR-0002 铁律：快照是唯一落盘真相，一切视图运行时重算、
--   永不落盘第二份；不建关系表存块间边）。存量课程为 NULL = 结构未生成
--   （Missing 合法空状态）；重跑生成即原子覆盖。课程行与 learning_goal
--   原样保留（终点只是目标语句的派生，原始目标仍在）。
--
-- 存量失败批次不受影响（learning_growth_batches.status 状态机不触碰）。
--
-- v3 contract: 删数据/删表/删列/加列，无物理外键、无触发器。

DELETE FROM learning_lesson_progress WHERE lesson_id IN
    (SELECT lesson_id FROM learning_course_endpoints);

DELETE FROM learning_lessons WHERE lesson_id IN
    (SELECT lesson_id FROM learning_course_endpoints);

DROP TABLE learning_course_endpoints;

ALTER TABLE learning_courses DROP COLUMN compass_md;
ALTER TABLE learning_courses DROP COLUMN compass_updated_at;

ALTER TABLE learning_courses ADD COLUMN structure_json TEXT CHECK (
    structure_json IS NULL
    OR (json_valid(structure_json) AND json_type(structure_json) = 'object')
);
