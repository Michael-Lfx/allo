-- 概念别名结构化升级（三轮 g2，j04）：aliases_json 元素从字符串升为轻量
-- 记录 {name, definition?, attestation?, status}——别名 = 概念的投影命名
-- 面，携带该名字的意义与出处；status = active/retired（退役不占名池，与
-- deprecated 口径衔接）。存量字符串逐个回填 {name, status:"active"}
-- （definition/attestation 缺席 = 与概念定义同义/出处未知）。
--
-- 同时给概念网边加 ref_name（节点引用该概念时使用的名字，可空）：跨课程
-- 反查按投影名分组的数据源（二轮 g2 引用名开讲的存储面），生长落库时写入。
-- 老行为名（空 = 未知，读侧回退概念正名）。

UPDATE learning_concept_registry
SET aliases_json = COALESCE((
    SELECT json_group_array(
        json_object('name', je.value, 'status', 'active')
    )
    FROM json_each(learning_concept_registry.aliases_json) je
), '[]');

ALTER TABLE learning_lesson_concepts ADD COLUMN ref_name TEXT;
