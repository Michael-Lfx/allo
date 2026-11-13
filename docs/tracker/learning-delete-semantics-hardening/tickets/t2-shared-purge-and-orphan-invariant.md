# t2: 双删除路径共享 purge + 清理清单补齐 + 零孤儿断言（B2）

## Status

已关（2026-11-13 施工收口，f8ab67bc5 经 merge b0ed32546 合入集成分支；整波经 04c110cae 并回 learn0928）。

## 已核验事实（2026-11-13 对码）

- 两条删除路径清理清单已漂移（`nomifun-learning/src/service/course.rs`）：
  - `delete_course`（软删，g3）：清 lesson_concepts / question_tags(course) /
    activities / lessons / modules / course_tags / basis_declarations /
    墓碑位。缺 `learning_lesson_sections`。
  - `delete_courses_by_title`（硬删，tutorial seed replace）：
    缺 `lesson_concepts`（g3 为修「孤儿课时把全局已教账顶高」污染加的，
    硬删没跟上）、缺 `basis_declarations`（m2 订正面）、缺
    `lesson_sections`；且硬删连课程行都删，孤儿边永远无主，比软删更糟。
  - m2 订正注释自称「同 delete_course」但只同步了 question_tags 顺序。
- 历史已两次咬人：g3 注释记载的孤儿课时污染事故；070 迁移被迫写专门
  孤儿清理段（「不保留孤儿日志」）。
- 硬删 docstring「concepts」一词已过时：070 后概念池
  （learning_concept_registry）用户级、无 course_id，课级概念清理已不可
  能也不需要——docstring 顺手修。
- dev 库当前三类孤儿（lesson_concepts / basis_declarations /
  lesson_sections）均为 0——本票纯预防，无存量清洗负担。
- 物理外键被合同禁用（`id_schema_contract::validate_no_physical_foreign_keys`），
  完整性只能由应用层删除清单保证——清单必须单源。

## Terms

- **R1（共享 purge）**：抽出单一
  `purge_course_content(tx, course_id)`（软删/硬删共用），清单 =
  lesson_sections（新补）→ lesson_concepts → question_tags(course) →
  activities → lessons → modules → course_tags → basis_declarations。
  软删在此基础上加墓碑位；硬删在此基础上加 enrollments/review_items/
  lesson_progress/attempts 清理 + 课程行硬删（现语义保持）。
- **R2（零孤儿断言）**：共享测试工具——两路径各跑一遍删后断言：
  八张内容表零孤儿（按 course_id / lesson_id / module_id / activity_id
  反查）。落点：tests.rs 现有删除测试（2457 墓碑断言、2626 硬删
  question_tags）旁，不另起文件。
- **R3（不做）**：账本面（xp_events/quiz_rounds/review_log/review_events/
  enrollment/progress/attempts 软删保留）语义不动（map 观察项：硬删路径
  的账本悬挂行同属 GC 观察项）；test seed `--fresh`/管理面行为不动；
  物理外键继续禁用（合同）。

## Acceptance

- 两路径删后八表零孤儿断言绿；`cargo test -p nomifun-learning` 全绿。
- 对码复查：`delete_courses_by_title` 与 `delete_course` 共用同一清单，
  差异只在账本面与课程行处置。

## Resolution

已施工收口（2026-11-13，f8ab67bc5，TDD 全程）。

- R1：模块级 `purge_course_content(transaction, course_id)` 单源落 course.rs，八表清单按票面顺序（lesson_sections 新补 → lesson_concepts → question_tags(course) → activities → lessons → modules → course_tags → basis_declarations），docstring 写明「先子后父」与「新增挂课内容行的表必须来此补 DELETE」约定；软删 = purge + 墓碑位（enabled=0 留给 t3 未碰），硬删 = per-enrollment 清理 + enrollments + 同一 purge + 课程行硬删。过时「concepts」docstring 已修。
- R2：共享种子 `seed_course_with_full_content`（八表各 1 行 + 种子自证）+ `assert_course_content_fully_purged`（course 直键三表等值计数 + lesson/activity 键定五表 NOT EXISTS 孤儿形态——免疫「子查询落已清空宿主表恒绿」陷阱）；两测试 `delete_course_soft_purge_leaves_zero_content_orphans` / `delete_by_title_hard_purge_leaves_zero_content_orphans` 落既有删除测试旁。Red 阶段失败点与票面已核验事实吻合（软删红在 sections、硬删红在 basis_declarations）。
- R3：账本面/外键/t3 范围零触碰（确认）。
- 验证：当波 968/0；整波终态 965/0（t1 删 5 例日历测试 + 增 1 例 memory 测试后）。
- 真机复验点：无（纯内部重构，行为由测试钉死）。
