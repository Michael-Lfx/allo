-- 课程级「现存且启用」口径（三轮 g3）：enabled 翻转 = 归档/恢复（数据零删
-- 零改，可逆暂停；删除仍走 069 软删墓碑不可逆）。活跃课程集单源 =
-- deleted_at IS NULL AND enabled = 1：安排面（到期队列/就绪推荐/教练入口/
-- 课时内容生成）只服务现存且启用的课程；历史面（日历/统计聚合/打卡历史）
-- 不过筛，已计入的学习记录如实保留。
ALTER TABLE learning_courses ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
