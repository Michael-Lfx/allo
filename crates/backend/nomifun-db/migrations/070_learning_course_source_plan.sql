-- ==== 069_learning_course_source_plan.sql ====
-- 学习课程 v2（t4）：`learning_courses` 增加来源计划快照列。
--
-- 一列承载建课时点的原子 JSON 信封（version + t1 清点/分诊 + t2 计划 +
-- t3 展开覆盖）：`CourseSourcePlanSnapshot { version, inventory, triage,
-- plan, coverage }`。字段演进靠信封里的 `version`（读侧对未知版本降级为
-- unavailable，不崩溃、不伪造空计划），不再为 v2 拆列。
--
-- Nullable、无 default：NULL = 旧课程/描述流/未启用新管线的课程——与空
-- 对象 {} 必须可区分，旧行不回填。写侧只在课程导入的同一事务写列（与
-- modules/lessons 同生共死，无半成品）；读侧只读快照，不实时重算。
-- 回滚 = 读侧忽略本列（迁移只加列，不删数据、不改写 068 及既有列）。
--
-- CHECK 仿 068 的 samples_meta_json：只放行 JSON 对象（信封恒为对象；
-- 数组/标量/非法 JSON 拒绝）。
ALTER TABLE learning_courses ADD COLUMN course_source_plan_json TEXT CHECK (
    course_source_plan_json IS NULL OR (
        json_valid(course_source_plan_json) AND json_type(course_source_plan_json) = 'object'
    )
);
