-- 目标感知采样（t2）：`learning_courses` 增加采样披露快照列。
--
-- 采样的既有两列各司其职、都不动：`samples_json` 承载 `Vec<(String,
-- String)>`（路径 → 摘录），既有消费方（课时生成上下文、课件邻居上下文）
-- 按该形状解析；本列是它的**伴随结构**——完整文件树（目录结构保留）、
-- 已采/未采计数、因低分未采的文件清单，供 UI 披露与诊断展示。
--
-- 目标文本本身不进本列：它复用 `learning_courses.learning_goal`
-- （048 已有，NOT NULL DEFAULT ''，旧行读出为空串），与学习图课程同列
-- 承载同类语义，因此本条迁移不涉目标列。
--
-- Nullable、无 default：NULL 表示"无目标路径（字母序回退）或旧课程"——
-- 与空对象 {} 必须可区分，旧课程快照继续按旧形状读取，不做回填。
ALTER TABLE learning_courses ADD COLUMN samples_meta_json TEXT CHECK (
    samples_meta_json IS NULL OR (
        json_valid(samples_meta_json) AND json_type(samples_meta_json) = 'object'
    )
);
