-- 多知识库（t4 评审修复）：`learning_courses.source_kb_ids` 登记进逻辑引用
-- 契约所需的具名索引。
--
-- 迁移 073 加了列表列，但漏了契约登记：`id_schema_contract.rs` 的 JSON 列表
-- 登记（`cs_agents.knowledge_base_ids` 先例，迁移 019）要求该列上有具名索引，
-- 登记条目本身才是一条活的 schema 断言（缺索引 = 启动即失败）。
-- 索引口径与先例逐字一致：普通列进索引（读侧 json_each 展开与孤儿审计按
-- 索引名引用），不是表达式索引。
CREATE INDEX idx_learning_courses_source_kb_ids_json
    ON learning_courses(source_kb_ids);
