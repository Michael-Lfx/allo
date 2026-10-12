-- 学习图课程目录删除改为软删除（ADR-0009 后续裁定的删除语义）：
-- 课程行保留为墓碑（名称留作标识、deleted_at 即删除状态位），课时/模块/
-- 概念网边按勾选物理删除；遗留数据凭 course_id 归账，清理时有据可查。
ALTER TABLE learning_courses ADD COLUMN deleted_at INTEGER;
