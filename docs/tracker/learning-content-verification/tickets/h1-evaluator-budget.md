---
id: h1
title: "verify_math 求值器预算硬化——表达式长度/嵌套深度防爆栈"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# h1: verify_math 求值器预算硬化

**What to build:** 给 allo 的 `verify_math` Pratt 求值器补两道上限——表达式原始字符数 `MAX_EXPR_LEN = 2048`、嵌套深度 `MAX_DEPTH = 64`；超限返回可读 `ExprError`，不 panic、不爆栈。正常表达式零行为变化。本票是 w2 的输入防护前置：w2 的数值 claim 直接来自模型，长/深表达式会进同一求值器。

## allo 现状（2026-11-02 实码）

- 求值器单源：`crates/backend/nomifun-learning/src/verify_math.rs`（1077 行）。
  - `eval_expr(expr, bindings)`（:115）→ `tokenize` → `Parser::parse_expr(0)`。
  - `Parser { tokens, pos, bindings, end }`（:300）**无 depth 字段**；`parse_expr`（:318）与 `parse_prefix`（:358）递归，括号/函数/一元负号都会加深调用栈。
  - `eval_expr` 入口**无长度检查**；`tokenize` 也不限长。
  - 现有错误面：`ExprError { position, message }`；词法/语法错带位置，模型可读。
  - 现有测试覆盖优先级/函数/容差/采样/identity，**无深嵌套/超长用例**。
- 消费方：agent 工具 op `verify_math`（`crates/backend/nomifun-ai-agent/src/lesson_content_loop.rs:1249-1290`）直调 `verify_claim`；`verify_eval` 与 `verify_identity` 都调 `eval_expr`。模型输入无外部长度约束（受 LLM 输出预算，但不能当安全边界）。

## learnhub 参考（`c14467e`，2026-11-02，P0 评审修复）

- `src-tauri/src/domain/verify_math/expr.rs`：`MAX_EXPR_LEN = 512`（原始 chars，归一化前；learnhub 原值——allo h1 取 2048，长度不是爆栈向量、深度闸才是，见实施要点）；`MAX_DEPTH = 64`；`parse_bp` 入口 `if self.depth >= MAX_DEPTH → ExprError::Syntax{msg:"嵌套过深(上限 64 层)"}`；`eval.rs` 另有 `eval_depth` 双护（learnhub 建 AST，解析与求值分离；allo 解析即求值，parser 一道护即可）。
- 测试：2000 层嵌套括号、2000 个一元负号、601 字符表达式、100 层嵌套（201 字符）全部报错不 abort；正常表达式不受影响。
- 该修复与 `%`/Unicode 归一化、容差量纲、claims 剥离同批；本票**只取预算**，其余不吸收。

## 实施要点

- [ ] `MAX_EXPR_LEN: usize = 2048`、`MAX_DEPTH: usize = 64` 常量（`verify_math.rs` 顶部，带注释说明来源与「模型输入不是安全边界」；长度取 2048 = 用户评审后的 allo 口径——512 会误伤多点求和/长展开，而爆栈由深度闸负责）。
- [ ] `eval_expr` 入口先查 `expr.chars().count() > MAX_EXPR_LEN`：返回 `ExprError`，`position` = 第 2049 个字符的字节偏移（`char_indices().nth(MAX_EXPR_LEN)`），文案点名「表达式过长（上限 2048 字符）——请拆分或化简」。
- [ ] `Parser` 增 `depth: usize`；`parse_expr` 拆成薄包装 + `parse_expr_inner`：
      ```rust
      fn parse_expr(&mut self, min_bp: u8) -> Result<f64, ExprError> {
          if self.depth >= MAX_DEPTH {
              return Err(ExprError { position: self.peek_position(), message: "表达式嵌套过深（上限 64 层）——请拆分或化简".into() });
          }
          self.depth += 1;
          let result = self.parse_expr_inner(min_bp);
          self.depth -= 1;
          result
      }
      ```
      确保所有早返回路径都经包装器减 depth（`parse_expr_inner` 内可继续早返回）。
- [ ] `peek_position()` 取当前 token 位置；表达式结束时用 `self.end`。错误文案与现有 `ExprError` 风格一致（中文、带位置）。
- [ ] 不改工具 schema/description（避免第四处复述；错误文案本身就是模型可见反馈）；不改 `MathClaim`/`NumericClaim` 契约；不改容差。
- [ ] 测试（`verify_math.rs` 的 `#[cfg(test)]`）：
  - `deep_parens_report_instead_of_overflow`：`"(".repeat(2000) + "1" + &")".repeat(2000)`（4001 字符）→ Err 不崩（先撞长度闸，文案「过长」）；预算内深嵌套 `"(".repeat(1000) + "1" + &")".repeat(1000)`（2001 字符）→ Err，消息含「嵌套过深」。
  - `deep_unary_reports`：2000 个 `-` + `1`（2001 字符，预算内）→ Err，消息含「嵌套过深」。
  - `long_expression_reports`：2049 字符（`"1+".repeat(1024) + "1"`）→ Err，消息含「过长」；601 字符平铺表达式照常求值（列入 `normal_expressions_unchanged`）。
  - `depth_64_boundary`：64 层通过 / 65 层报错（按实现口径定，测试钉死）。
  - `normal_expressions_unchanged`：现有用例不回归；`(1+2)*3`、`sqrt(2)`、`2^3^2`、601 字符平铺表达式等。
- [ ] 保持 `verify_eval`/`verify_identity` 的 `Fail` 语义不变：预算错误仍是 `ClaimVerdict::Fail`（解析错 = Fail，不是工具 Err），模型可据此修正 claim。

## 验收

- `cargo test -p nomifun-learning verify_math` 全绿；新增预算用例真的在默认测试栈上不崩（无 guard 时会栈溢出，测试即证明）。
- `cargo test -p nomifun-ai-agent lesson_content_loop` 全绿（工具 handler 直调/全循环样不回归）。
- 正常表达式输出逐位不变（现有断言全绿）。
- 零迁移、零 UI、零 API 变更。

## 非目标

- `%` 后缀/Unicode 归一化（learnhub 同批但不属于预算；w2 用 `unit=percent` 表达百分数，不需要 `%` 词法）。
- 三值 verdict / unverifiable（allo 工具面非硬闸，保持 Pass/Fail）。
- identity 按表达式派生种子 / 采样域调整（allo 固定 LCG 7 点 `[-3,3)` 现状不动）。
- claims 围栏、容差量纲、strip 字节守恒（learnhub c14467e 的其余项；allo 无对应载体）。
- 新增外部输入长度限制的 HTTP 层（求值器入口足够）。

## 依赖与编排

- 前置：无（可独立先行）。
- 后继：w2 依赖本票（数值 claim 表达式直接进求值器；预算先落地）。
- 分支建议：`feat/verify-math-budget`；若与 w2 同波，h1 先合、w2 紧随。
