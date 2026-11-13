# d1: 删除课程对话框——去掉逐字输入确认 + 修正归档理由文案

## Status

施工中（2026-11-13 开票；用户桌面走查指名，附截图）。

## 已核验事实（2026-11-13 对库）

- 对话框实现 = `CourseCard.tsx` `CourseDeleteDialog`：确认按钮被
  `confirmation.trim() === course.title.trim()` 门死，占位提示键
  `deleteCourseRequireInput`、不匹配提示键 `deleteCourseMismatchHint`。
  这是三轮 g3 的强确认形态，用户判定为多余摩擦。
- 归档语义 = `nomifun-learning/src/service/course.rs` `set_course_archived`：
  只翻 `enabled` 位，零数据丢失，从复习队列/推荐等安排面消失，恢复即回来。
- 删除语义 = 同文件 `delete_course`：课时/模块/概念网边/活动/题库标签/声明锚
  物理删除，课程行留墓碑（`deleted_at`），复习卡/作答/进度/注册保留，
  **无恢复路径**。
- 文案失真点：第三条后果「只是想暂停这门课的话，请改用归档」——但第二条
  后果已明说删除同样会让本课离开复习队列和推荐（进度保留），即删除本身就是
  「暂停」的效果；归档的真实差异是**课程本身完整保留、随时可恢复**，
  与删除的不可恢复相对。现文案把归档的理由说成了删除已具备的效果。

## Terms

- **R1（交互降噪）**：去掉逐字输入确认——`CourseDeleteDialog` 移除
  `Input` 与不匹配提示、移除 `confirmed` 门；确认按钮常态可用。键
  `deleteCourseRequireInput` / `deleteCourseMismatchHint` 双语删除，
  `gen:i18n` 后回归。
- **R2（文案修正）**：`deleteCourseConsequenceIrreversible` 改为如实对照：
  zh「删除无法恢复——想保留课程本身、以后随时恢复继续学，请改用「归档」。」
  en "Deletion cannot be undone — to keep the course intact and restore it
  anytime, archive it instead."（归档与删除的真实差异 = 课程本身保留 +
  可逆；其余两条后果句不动。）
- **R3（不做）**：对话框结构与其余后果句不动（三条后果如实且已口语化）；
  后端删除/归档语义不动（无 API 变更）；归档入口/恢复入口不动；
  `deleteCourseHint` / `deleteCourseConsequenceContent` /
  `deleteCourseConsequenceRecords` 三键不动。

## Acceptance

- 双语键集对称（删除两键后 `check:i18n` 绿）；typecheck 绿；learning 相关
  测试绿（settingsCopyPlain 只断 `deleteCourseConsequenceRecords`，不在
  波及面）。
- 真机：删除弹窗无输入框、确认按钮直接可点；三条后果文案如实。

## Resolution

已施工收口（2026-11-13，68b949146 经 merge 合入 learn0928）。

- R1：`CourseDeleteDialog` 移除逐字输入 `Input`、不匹配提示段与 `confirmed`
  门，危险确认按钮常态可用；键 `deleteCourseRequireInput` /
  `deleteCourseMismatchHint` 双语删除，`gen:i18n` 重生成（12441 键）。
- R2：`deleteCourseConsequenceIrreversible` 双语改为如实对照——zh「删除无法
  恢复——想保留课程本身、以后随时恢复继续学，请改用「归档」。」en
  "Deletion cannot be undone — to keep the course intact and restore it
  anytime, archive it instead."（归档的真实差异 = 课程本身保留 + 可逆。）
- 验证：typecheck 绿；check:i18n 绿；learning 408 测试全绿
  （settingsCopyPlain 只断 `deleteCourseConsequenceRecords`，未波及）；
  全库 grep 无两键残留引用。
- 真机复验点：删除弹窗无输入框、确认直接可点；第三条后果文案如实对照
  归档/删除。
