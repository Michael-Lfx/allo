//! verify_math — 确定性数学断言核验器（票 w1，蓝图
//! `docs/tracker/learning-content-verification/blueprints/2026-10-31-w1-verify-math-v0.md`
//! 4.1/A1/A2）。
//!
//! 断言接口优先于解算器：模型负责把正文数学翻译成 claim JSON（eval /
//! identity 两种），本模块是纯确定性代码——eval = 手写 Pratt 求值器实算
//! 表达式，identity = 等式两边在固定域内以固定种子 LCG 采样数值比对（零
//! rand 依赖、可复现）。核验失败是正常结论（[`ClaimVerdict::Fail`] 带死因
//! 与对照值，模型据此自修改），只有 claim JSON 本体损坏才在工具层转 Err。
//!
//! 分层（A1）：求值器单源落 nomifun-learning——ai-agent 的工具 handler 直调
//! （单向依赖已核实），w2 quiz 线与未来消费方复用最近。零新依赖：解析器
//! 手写（全仓无 evalexpr/meval，AGENTS 依赖纪律）。

use serde::Deserialize;

/// 相对容差缺省（A2，对齐 progress.rs Numeric 容差先例精神）。
const DEFAULT_REL_TOLERANCE: f64 = 1e-9;
/// 绝对容差地板（A2）：极小期望值下相对容差塌缩时的保底比较阈。
const ABS_TOLERANCE_FLOOR: f64 = 1e-12;
/// identity 缺省采样点数（蓝图 4.1；未决事项「实现可调常量」）。
const IDENTITY_DEFAULT_SAMPLES: usize = 7;
/// 采样补采上限（蓝图 4.1：跳过补采至多 3×samples——奇点邻域防死循环）。
const IDENTITY_RESAMPLE_CAP_FACTOR: usize = 3;
/// identity 缺省采样域（蓝图 4.1/未决事项：[−3, 3)）。
const IDENTITY_DOMAIN_LOW: f64 = -3.0;
const IDENTITY_DOMAIN_HIGH: f64 = 3.0;

/// 模型提交的数学断言（工具 op 的入参契约，serde 形状即 wire 契约）。
/// `tolerance` 是相对容差（× max(1, |期望|) 的比较尺度）；缺席 = 缺省
/// 1e-9。未知字段宽容忽略（w1 评审修复：全仓工具 op 参数均容多余字段，
/// 模型多带一字段不该吃 Err 多耗一轮自纠——字段名拼错仍会因必填槽缺席
/// 当场可见）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MathClaim {
    /// 求值断言：实算 `expr`，有 `expected` 时按容差比对，无则只回读实算值。
    Eval {
        expr: String,
        #[serde(default)]
        expected: Option<f64>,
        #[serde(default)]
        tolerance: Option<f64>,
    },
    /// 恒等断言：`lhs`/`rhs` 在 `variables` 的固定域采样点上数值相等
    /// （相对容差）。`samples` 缺省 7 点。
    Identity {
        lhs: String,
        rhs: String,
        #[serde(default)]
        variables: Vec<String>,
        #[serde(default)]
        tolerance: Option<f64>,
        #[serde(default)]
        samples: Option<u8>,
    },
}

/// 核验结论（工具 op 把它序列化回给模型；`kind` 标签让 verdict JSON 自明）。
/// `Fail` 的三个槽位按结论类型择情填充：解析错 = reason 定位；eval 比对失败
/// = computed 带实算值；identity 采样失败 = counterexample 带反例点。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimVerdict {
    Pass {
        /// eval 的实算值；identity 恒等无单值 → None。
        value: Option<f64>,
    },
    Fail {
        /// 死因（对模型可读：位置 + 原因 / 期望 vs 实算）。
        reason: String,
        /// eval 的实算值（求值本身非有限时为 None）。
        computed: Option<f64>,
        /// identity 反例：变量取值与两侧值。
        counterexample: Option<String>,
    },
}

fn pass(value: Option<f64>) -> ClaimVerdict {
    ClaimVerdict::Pass { value }
}

fn fail(reason: impl Into<String>) -> ClaimVerdict {
    ClaimVerdict::Fail {
        reason: reason.into(),
        computed: None,
        counterexample: None,
    }
}

/// 表达式错误：带字节偏移位置（对模型可读的定位面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    /// 错误记号的字节偏移（0 基；表达式末尾 = `expr.len()`）。
    pub position: usize,
    pub message: String,
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "位置 {}: {}", self.position, self.message)
    }
}

/// 求值一个表达式（零依赖手写 Pratt，蓝图 4.1 文法）。
///
/// 文法：数字（整/浮/科学计数）、标识符（`bindings` 里的变量、常量
/// `pi`/`e`）、一元 `-`、二元 `+ - * / ^`（^ 右结合、高于一元负号）、括号、
/// 单参函数 `sqrt sin cos tan ln log exp abs`（log = 以 10 为底）。**不支持
/// 隐式乘法与 % 字面量**——报错文案被红绿样钉住（模型必须写显式 `*` 与
/// 小数）。
///
/// 求值按 f64 语义：除零/ln 负数/溢出产生非有限值**原样返回**（不 panic、
/// 不在此处报错）——非有限的失败路径由核验语义层处理。
pub fn eval_expr(expr: &str, bindings: &[(String, f64)]) -> Result<f64, ExprError> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        bindings,
        end: expr.len(),
    };
    let value = parser.parse_expr(0)?;
    if parser.pos < tokens.len() {
        let (position, message) = trailing_token_error(&tokens, parser.pos);
        return Err(ExprError { position, message });
    }
    Ok(value)
}

/// 顶层剩余记号的错误定位（多余右括号、缺运算符的并置等）。
fn trailing_token_error(tokens: &[(Token, usize)], pos: usize) -> (usize, String) {
    let (token, position) = &tokens[pos];
    match token {
        Token::RParen => (
            *position,
            "多余的右括号——缺少与它配对的左括号".to_owned(),
        ),
        _ => (*position, format!("此处有多余内容「{token:?}」")),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
}

/// 词法：数字 / 标识符 / 运算符 / 括号；空白跳过；`%` 与其它字符按蓝图
/// 钉死的文案报错（位置 = 该字符的字节偏移）。
fn tokenize(expr: &str) -> Result<Vec<(Token, usize)>, ExprError> {
    let mut tokens = Vec::new();
    let mut chars = expr.char_indices().peekable();
    while let Some(&(index, ch)) = chars.peek() {
        match ch {
            ' ' | '\t' | '\r' | '\n' => {
                chars.next();
            }
            '+' => {
                tokens.push((Token::Plus, index));
                chars.next();
            }
            '-' => {
                tokens.push((Token::Minus, index));
                chars.next();
            }
            '*' => {
                tokens.push((Token::Star, index));
                chars.next();
            }
            '/' => {
                tokens.push((Token::Slash, index));
                chars.next();
            }
            '^' => {
                tokens.push((Token::Caret, index));
                chars.next();
            }
            '(' => {
                tokens.push((Token::LParen, index));
                chars.next();
            }
            ')' => {
                tokens.push((Token::RParen, index));
                chars.next();
            }
            '%' => {
                return Err(ExprError {
                    position: index,
                    message: "不支持 % 字面量（百分比请写成小数，例如 5% 写成 0.05）".into(),
                });
            }
            '0'..='9' | '.' => {
                let token = scan_number(expr, &mut chars, index)?;
                tokens.push(token);
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let token = scan_ident(&mut chars, index);
                tokens.push(token);
            }
            _ => {
                return Err(ExprError {
                    position: index,
                    message: format!("不支持的字符「{ch}」——表达式只允许数字、+ - * / ^ ( ) 与英文名"),
                });
            }
        }
    }
    Ok(tokens)
}

/// 数字扫描：整/浮 + 可选科学计数（`e`/`E` [+-] 数字）。 Malformed 数字
/// （`1.2.3`、`1e`）按词法错处理。
fn scan_number(
    expr: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Result<(Token, usize), ExprError> {
    let bytes = expr.as_bytes();
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        let mut cursor = end + 1;
        if cursor < bytes.len() && (bytes[cursor] == b'+' || bytes[cursor] == b'-') {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            end = cursor;
        }
    }
    let text = &expr[start..end];
    // 跳过词法面已消费的字符（按字节推进迭代器）。
    for _ in start..end {
        chars.next();
    }
    let value: f64 = text.parse().map_err(|_| ExprError {
        position: start,
        message: format!("无法解析的数字「{text}」"),
    })?;
    Ok((Token::Number(value), start))
}

/// 标识符扫描：`[A-Za-z_][A-Za-z0-9_]*`。
fn scan_ident(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, start: usize) -> (Token, usize) {
    let mut name = String::new();
    while let Some(&(_, ch)) = chars.peek() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    (Token::Ident(name), start)
}

/// 单参函数表（蓝图 4.1；log = 以 10 为底）。
fn function(name: &str) -> Option<fn(f64) -> f64> {
    match name {
        "sqrt" => Some(f64::sqrt),
        "sin" => Some(f64::sin),
        "cos" => Some(f64::cos),
        "tan" => Some(f64::tan),
        "ln" => Some(f64::ln),
        "log" => Some(f64::log10),
        "exp" => Some(f64::exp),
        "abs" => Some(f64::abs),
        _ => None,
    }
}

const AVAILABLE_FUNCTIONS: &str = "sqrt sin cos tan ln log exp abs";

/// 二元运算的结合性与优先级（l_bp, r_bp）：+ − 左结合；^ 右结合且优先级
/// 高于乘除与一元负号（`-2^2 = -(2^2)`、`2^-3` 合法）。
fn infix_binding_power(token: &Token) -> Option<(u8, u8)> {
    match token {
        Token::Plus | Token::Minus => Some((1, 2)),
        Token::Star | Token::Slash => Some((3, 4)),
        Token::Caret => Some((7, 6)),
        _ => None,
    }
}

/// 一元负号的前缀结合力：高于乘除（`-2*3 = (-2)*3`）、低于 ^（`-2^2 =
/// -(2^2)`，蓝图「^ 高于一元负号」）。
const UNARY_BINDING_POWER: u8 = 5;

struct Parser<'a> {
    tokens: &'a [(Token, usize)],
    pos: usize,
    bindings: &'a [(String, f64)],
    end: usize,
}

impl<'a> Parser<'a> {
    /// 取当前记号的持有副本（Token 克隆廉价——名字都是短串），避免借用
    /// self 与 pos 推进互相干扰。
    fn peek_owned(&self) -> Option<(Token, usize)> {
        self.tokens
            .get(self.pos)
            .map(|(token, position)| (token.clone(), *position))
    }

    /// Pratt 主循环：先取前缀项，然后按结合力消费二元运算。紧跟在 primary
    /// 之后的 primary 记号 = 隐式乘法，按蓝图钉死的文案报错。
    fn parse_expr(&mut self, min_bp: u8) -> Result<f64, ExprError> {
        let mut lhs = self.parse_prefix()?;
        loop {
            let Some((token, position)) = self.peek_owned() else {
                break;
            };
            match token {
                Token::Number(_) | Token::Ident(_) | Token::LParen => {
                    return Err(ExprError {
                        position,
                        message: "不支持隐式乘法（省略的乘号）——请写显式 *，例如 2*x、x*(y+1)".into(),
                    });
                }
                _ => {}
            }
            let Some((l_bp, r_bp)) = infix_binding_power(&token) else {
                break;
            };
            if l_bp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.parse_expr(r_bp)?;
            lhs = match token {
                Token::Plus => lhs + rhs,
                Token::Minus => lhs - rhs,
                Token::Star => lhs * rhs,
                Token::Slash => lhs / rhs,
                Token::Caret => lhs.powf(rhs),
                _ => unreachable!("infix_binding_power 已过滤"),
            };
        }
        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<f64, ExprError> {
        let Some((token, position)) = self.peek_owned() else {
            return Err(ExprError {
                position: self.end,
                message: "表达式意外结束——缺操作数".into(),
            });
        };
        match token {
            Token::Minus => {
                self.pos += 1;
                let value = self.parse_expr(UNARY_BINDING_POWER)?;
                Ok(-value)
            }
            Token::Plus => Err(ExprError {
                position,
                message: "不支持一元 +——正数直接写数字即可".into(),
            }),
            Token::Number(value) => {
                self.pos += 1;
                Ok(value)
            }
            Token::Ident(name) => self.parse_ident(&name, position),
            Token::LParen => {
                self.pos += 1;
                let value = self.parse_expr(0)?;
                match self.peek_owned() {
                    Some((Token::RParen, _)) => {
                        self.pos += 1;
                        Ok(value)
                    }
                    _ => Err(ExprError {
                        position: self.end,
                        message: "缺少右括号——「(」没有配对的「)」".into(),
                    }),
                }
            }
            Token::Star | Token::Slash | Token::Caret | Token::RParen => Err(ExprError {
                position,
                message: "此处缺少左操作数（运算符或右括号出现在了表达式开头）".into(),
            }),
        }
    }

    /// 标识符：跟 `(` = 函数调用；否则变量（bindings 优先）或常量 pi/e。
    fn parse_ident(&mut self, name: &str, position: usize) -> Result<f64, ExprError> {
        self.pos += 1;
        if matches!(self.peek_owned(), Some((Token::LParen, _))) {
            let Some(func) = function(name) else {
                return Err(ExprError {
                    position,
                    message: format!("未知函数「{name}」——可用函数：{AVAILABLE_FUNCTIONS}"),
                });
            };
            self.pos += 1;
            let arg = self.parse_expr(0)?;
            match self.peek_owned() {
                Some((Token::RParen, _)) => {
                    self.pos += 1;
                }
                _ => {
                    return Err(ExprError {
                        position: self.end,
                        message: format!("函数 {name} 缺少右括号"),
                    });
                }
            }
            return Ok(func(arg));
        }
        if let Some((_, value)) = self.bindings.iter().find(|(key, _)| key == name) {
            return Ok(*value);
        }
        match name {
            "pi" => Ok(std::f64::consts::PI),
            "e" => Ok(std::f64::consts::E),
            _ => Err(ExprError {
                position,
                message: format!(
                    "未知变量「{name}」——identity claim 请把它的名字放进 variables，或使用常量 pi/e"
                ),
            }),
        }
    }
}

/// claim 核验入口（蓝图 4.1：永不 panic；解析错也算核验结论——解析失败是
/// `Fail` verdict 而非 Err，只有 claim JSON 本体损坏才在 op 层 Err）。
pub fn verify_claim(claim: &MathClaim) -> ClaimVerdict {
    match claim {
        MathClaim::Eval {
            expr,
            expected,
            tolerance,
        } => verify_eval(expr, *expected, *tolerance),
        MathClaim::Identity {
            lhs,
            rhs,
            variables,
            tolerance,
            samples,
        } => verify_identity(lhs, rhs, variables, *tolerance, *samples),
    }
}

/// 容差换算（蓝图 4.1/A2）：相对容差 × 比较尺度，绝对地板 1e-12 兜底。
fn effective_tolerance(tolerance: Option<f64>, scale: f64) -> f64 {
    let relative = tolerance.unwrap_or(DEFAULT_REL_TOLERANCE);
    (relative * scale.max(1.0)).max(ABS_TOLERANCE_FLOOR)
}

/// 容差槽位形状校验：负数 / 非有限 = claim 配置错（如实 Fail，不静默裁缺省）。
fn tolerance_is_valid(tolerance: Option<f64>) -> bool {
    tolerance.is_none_or(|value| value.is_finite() && value >= 0.0)
}

fn verify_eval(expr: &str, expected: Option<f64>, tolerance: Option<f64>) -> ClaimVerdict {
    if !tolerance_is_valid(tolerance) {
        return fail("tolerance 必须是非负有限数（相对容差）");
    }
    let value = match eval_expr(expr, &[]) {
        Ok(value) => value,
        Err(error) => return fail(format!("表达式「{expr}」无法求值：{error}")),
    };
    if !value.is_finite() {
        return ClaimVerdict::Fail {
            reason: format!(
                "表达式「{expr}」求值结果非有限（除零、定义域外或溢出——检查分母与函数定义域）"
            ),
            computed: None,
            counterexample: None,
        };
    }
    match expected {
        // 无期望值 = 求值回读：模型先用它拿实算值，再自己决定怎么写正文。
        None => pass(Some(value)),
        Some(expected) => {
            if !expected.is_finite() {
                return fail("expected 必须是有限数");
            }
            let tol = effective_tolerance(tolerance, expected.abs());
            if (value - expected).abs() <= tol {
                pass(Some(value))
            } else {
                ClaimVerdict::Fail {
                    reason: format!("实算值 {value} 与期望值 {expected} 超出容差（比较阈 {tol}）——请以实算值修正正文"),
                    computed: Some(value),
                    counterexample: None,
                }
            }
        }
    }
}

/// 固定种子 LCG（A2：确定性采样、零 rand 依赖、真机可复跑对账）。
/// 参数取 Knuth MMIX 31 2 64 常量；种子是任意固定常数（序列被单测钉住）。
struct Lcg {
    state: u64,
}

impl Lcg {
    const SEED: u64 = 0x00BD10ED_AB0BB608;

    fn new() -> Self {
        Self { state: Self::SEED }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// [0, 1) 内的确定性样本（53 位精度）。
    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 缺省域 [−3, 3) 内的确定性样本。
    fn next_sample(&mut self) -> f64 {
        IDENTITY_DOMAIN_LOW + (IDENTITY_DOMAIN_HIGH - IDENTITY_DOMAIN_LOW) * self.next_unit()
    }
}

fn render_bindings(bindings: &[(String, f64)]) -> String {
    bindings
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn verify_identity(
    lhs: &str,
    rhs: &str,
    variables: &[String],
    tolerance: Option<f64>,
    samples: Option<u8>,
) -> ClaimVerdict {
    if !tolerance_is_valid(tolerance) {
        return fail("tolerance 必须是非负有限数（相对容差）");
    }
    // 去重保序：重复变量只采一次（采样点数不被重复名单虚增）。
    let mut names: Vec<&str> = Vec::new();
    for name in variables {
        if !names.contains(&name.as_str()) {
            names.push(name.as_str());
        }
    }
    let point_count = samples
        .map(|value| value as usize)
        .unwrap_or(IDENTITY_DEFAULT_SAMPLES)
        .max(1);
    let mut lcg = Lcg::new();
    let mut valid = 0usize;
    let mut skipped = 0usize;
    let mut attempts = 0usize;
    let cap = point_count * IDENTITY_RESAMPLE_CAP_FACTOR;
    while valid < point_count && attempts < cap {
        attempts += 1;
        let bindings: Vec<(String, f64)> = names
            .iter()
            .map(|name| ((*name).to_owned(), lcg.next_sample()))
            .collect();
        let (left, right) = match (eval_expr(lhs, &bindings), eval_expr(rhs, &bindings)) {
            (Ok(left), Ok(right)) => (left, right),
            (Err(error), _) | (_, Err(error)) => {
                return fail(format!("等式两侧无法在采样点求值：{error}"));
            }
        };
        // 奇点/溢出点跳过并补采（蓝图 4.1）；表达式级解析错已在上一臂报出。
        if !left.is_finite() || !right.is_finite() {
            skipped += 1;
            continue;
        }
        valid += 1;
        let tol = effective_tolerance(tolerance, left.abs().max(right.abs()));
        if (left - right).abs() > tol {
            return ClaimVerdict::Fail {
                reason: format!(
                    "采样核验失败：等式在 {} 处不成立（lhs = {lhs}，rhs = {rhs}）",
                    render_bindings(&bindings)
                ),
                computed: None,
                counterexample: Some(format!(
                    "{} 时 lhs={} rhs={}",
                    render_bindings(&bindings),
                    left,
                    right
                )),
            };
        }
    }
    // 有效点 == 0 才 Fail（w1 评审修复：旧 `valid < IDENTITY_MIN_VALID_POINTS`
    // 把模型显式传 samples:1|2（wire 契约允许）且恒等式成立的合法低点数核验
    // 误判成 Fail——点实为有限且相等，文案「{valid}/{point_count} 个点两侧
    // 非有限」失实）。文案只报真实非有限计数。
    if valid == 0 {
        return fail(format!(
            "无有效采样点（{skipped} 个采样点两侧非有限——检查表达式定义域与奇点）"
        ));
    }
    pass(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(expr: &str) -> f64 {
        eval_expr(expr, &[]).expect(expr)
    }

    fn eval_err(expr: &str) -> ExprError {
        eval_expr(expr, &[]).expect_err(expr)
    }

    fn verify(claim: &MathClaim) -> ClaimVerdict {
        verify_claim(claim)
    }

    fn eval_claim(expr: &str) -> MathClaim {
        MathClaim::Eval {
            expr: expr.into(),
            expected: None,
            tolerance: None,
        }
    }

    fn identity_claim(lhs: &str, rhs: &str, variables: &[&str]) -> MathClaim {
        MathClaim::Identity {
            lhs: lhs.into(),
            rhs: rhs.into(),
            variables: variables.iter().map(|name| (*name).into()).collect(),
            tolerance: None,
            samples: None,
        }
    }

    // ── 求值器：全运算符 / 优先级 / 右结合 ^ / 一元负号 / 函数表 ────────

    #[test]
    fn operators_and_precedence() {
        assert_eq!(eval("1+2*3"), 7.0);
        assert_eq!(eval("(1+2)*3"), 9.0);
        assert_eq!(eval("10/4"), 2.5);
        assert_eq!(eval("10-2-3"), 5.0, "- 左结合");
        assert_eq!(eval("2+3*4^2"), 50.0, "^ 高于乘、乘高于加");
        assert!((eval("2*(3+4)/7") - 2.0).abs() < 1e-12);
    }

    #[test]
    fn caret_is_right_associative_and_outranks_unary_minus() {
        assert_eq!(eval("2^3^2"), 512.0, "右结合：2^(3^2)");
        assert_eq!(eval("-2^2"), -4.0, "^ 高于一元负号：-(2^2)");
        assert_eq!(eval("(-2)^2"), 4.0);
        assert_eq!(eval("2^-2"), 0.25, "^ 右侧允许一元负号");
        assert_eq!(eval("-2*-3"), 6.0);
        assert_eq!(eval("5-3"), 2.0, "二元减不被一元语义吞噬");
    }

    #[test]
    fn scientific_notation_and_constants() {
        assert_eq!(eval("1e3"), 1000.0);
        assert_eq!(eval("2.5e-2"), 0.025);
        assert_eq!(eval("1E2"), 100.0);
        assert!((eval("pi") - std::f64::consts::PI).abs() < 1e-15);
        assert!((eval("e") - std::f64::consts::E).abs() < 1e-15);
        assert!((eval("e^1") - std::f64::consts::E).abs() < 1e-15);
    }

    #[test]
    fn function_table() {
        assert_eq!(eval("sqrt(9)"), 3.0);
        assert_eq!(eval("abs(-2.5)"), 2.5);
        assert_eq!(eval("cos(0)"), 1.0);
        assert_eq!(eval("tan(0)"), 0.0);
        assert!((eval("sin(pi/2)") - 1.0).abs() < 1e-12);
        assert!((eval("ln(e)") - 1.0).abs() < 1e-12);
        assert!((eval("log(1000)") - 3.0).abs() < 1e-12, "log = 以 10 为底");
        assert!((eval("exp(0)") - 1.0).abs() < 1e-15);
        assert!((eval("sqrt(2)^2") - 2.0).abs() < 1e-12);
        assert!((eval("2*sqrt(3)") - (2.0 * 3.0f64.sqrt())).abs() < 1e-12);
    }

    #[test]
    fn non_finite_values_flow_through_without_panic() {
        assert!(eval("1/0").is_infinite(), "除零 = 非有限，不 panic");
        assert!(eval("0/0").is_nan());
        assert!(eval("ln(-1)").is_nan(), "ln 负数 = NaN");
        assert!(eval("sqrt(-1)").is_nan());
        assert!(eval("1e308*10").is_infinite(), "溢出 = 非有限");
    }

    #[test]
    fn variable_bindings_resolve_before_constants() {
        let bindings = vec![("x".to_owned(), 2.5f64)];
        assert_eq!(eval_expr("x*2", &bindings).expect("bindings"), 5.0);
        assert_eq!(eval_expr("x+x^2", &bindings).expect("bindings"), 2.5 + 6.25);
        let error = eval_err("x+1");
        assert!(error.message.contains("未知变量"), "{error}");
    }

    // ── 报错文案（蓝图钉死：隐式乘法与 % 必须报错，带位置） ─────────────

    #[test]
    fn implicit_multiplication_is_rejected_with_pinned_message() {
        for expr in ["2x", "(1+2)(3+4)", "2sqrt(3)", "2(3)"] {
            let error = eval_err(expr);
            assert!(
                error.message.contains("不支持隐式乘法"),
                "expr {expr:?} must pin the implicit-multiplication message: {error}"
            );
            assert!(error.message.contains("显式 *"), "{error}");
        }
        // 标识符并置（变量在场时也拒绝隐式乘法）。
        let bindings = vec![("x".to_owned(), 1.0f64), ("y".to_owned(), 2.0f64)];
        let error = eval_expr("x y", &bindings).expect_err("x y");
        assert!(error.message.contains("不支持隐式乘法"), "{error}");
        // 位置指向第二个操作数的起点。
        assert_eq!(eval_err("2x").position, 1);
    }

    #[test]
    fn percent_literal_is_rejected_with_pinned_message() {
        for expr in ["5%", "50%*2", "1+%"] {
            let error = eval_err(expr);
            assert!(
                error.message.contains("不支持 % 字面量"),
                "expr {expr:?} must pin the percent message: {error}"
            );
            assert!(error.message.contains("0.05"), "{error}");
        }
        assert_eq!(eval_err("5%").position, 1);
    }

    #[test]
    fn syntax_errors_carry_positions() {
        assert!(eval_err("2+").message.contains("意外结束"));
        assert_eq!(eval_err("2+").position, 2);
        assert!(eval_err("(1+2").message.contains("缺少右括号"));
        assert!(eval_err("nope(1)").message.contains("未知函数"));
        assert!(eval_err("*3").message.contains("缺少左操作数"));
        assert!(eval_err("(1+2))").message.contains("多余的右括号"));
        assert!(eval_err("1.2.3").message.contains("无法解析的数字"));
        assert!(eval_err("3美元").message.contains("不支持的字符"));
    }

    // ── a3 病例与 smoke oracle 期望值对账（蓝图 4.6：期望值算死） ────────

    #[test]
    fn a3_case_sum_of_squares() {
        // a3 病例形态：小数平方和是 token 级算术最不稳形态。
        let a3 = eval("1/(4-1)*((3.0-0.0)^2+(1.0-0.0)^2+(2.0-0.0)^2+(4.0-0.0)^2)");
        assert!((a3 - 10.0).abs() <= 1e-9, "a3 型平方和/3 = 10，got {a3}");
        // 真机病历：对照列平方和实算 0.16+0.16+0.09+0.09 = 0.50（模型曾写 ≈0.7）。
        let sum = eval("(0.4-0.0)^2+(0.4-0.0)^2+(0.3-0.0)^2+(0.3-0.0)^2");
        assert!(
            (sum - 0.5).abs() <= 1e-9 * 0.5f64.max(1.0),
            "a3 病例平方和 = 0.5，got {sum}"
        );
    }

    #[test]
    fn smoke_oracle_expected_values() {
        // smoke 种子序列（蓝图 4.6 双保险）的期望值算死：均值 0、平方和 30、
        // /3 = 10、σ = √10 ≈ 3.1623——判据比对「3.16」/「3.2」字符串变体。
        let squares = eval("3.0^2+1.0^2+2.0^2+4.0^2");
        assert_eq!(squares, 30.0);
        let variance = eval("1/(4-1)*(3.0^2+1.0^2+2.0^2+4.0^2)");
        assert_eq!(variance, 10.0);
        let sigma = eval("sqrt(1/(4-1)*(3.0^2+1.0^2+2.0^2+4.0^2))");
        assert!((sigma - 3.1623).abs() < 1e-4, "σ = √10 ≈ 3.1623，got {sigma}");
        // 均值 0（对照面）。
        let mean = eval("(3.0-1.0+2.0-4.0)/4");
        assert_eq!(mean, 0.0);
    }

    // ── claim 核验语义：eval ────────────────────────────────────────────

    #[test]
    fn eval_without_expected_passes_with_computed_value() {
        let outcome = verify(&MathClaim::Eval {
            expr: "1+1".into(),
            expected: None,
            tolerance: None,
        });
        assert_eq!(outcome, pass(Some(2.0)));
    }

    #[test]
    fn eval_with_expected_uses_relative_tolerance() {
        let pass_claim = MathClaim::Eval {
            expr: "1+1".into(),
            expected: Some(2.0),
            tolerance: None,
        };
        assert_eq!(verify(&pass_claim), pass(Some(2.0)));
        // 默认容差 1e-9：0.2 的偏差必红。
        let fail_claim = MathClaim::Eval {
            expr: "1+1".into(),
            expected: Some(2.2),
            tolerance: None,
        };
        match verify(&fail_claim) {
            ClaimVerdict::Fail {
                reason,
                computed,
                counterexample,
            } => {
                assert!(reason.contains("2") && reason.contains("2.2"), "{reason}");
                assert_eq!(computed, Some(2.0));
                assert_eq!(counterexample, None);
            }
            other => panic!("expected Fail, got {other:?}"),
        }
        // 相对容差按期望值缩放：tolerance 0.1 × max(1, 2.2) = 0.22 ≥ 0.2 → Pass。
        let loose_claim = MathClaim::Eval {
            expr: "1+1".into(),
            expected: Some(2.2),
            tolerance: Some(0.1),
        };
        assert_eq!(verify(&loose_claim), pass(Some(2.0)));
    }

    #[test]
    fn eval_parse_errors_are_fail_verdicts_not_tool_errors() {
        // 蓝图 4.1：解析错也算核验结论（模型据此改 claim），只有 JSON 本体
        // 损坏才在 op 层 Err——这里钉「Fail 携带定位死因」的形状。
        match verify(&eval_claim("2x")) {
            ClaimVerdict::Fail { reason, .. } => {
                assert!(reason.contains("不支持隐式乘法"), "{reason}");
                assert!(reason.contains("位置"), "{reason}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
        match verify(&eval_claim("50%*2")) {
            ClaimVerdict::Fail { reason, .. } => assert!(reason.contains("不支持 % 字面量"), "{reason}"),
            other => panic!("expected Fail, got {other:?}"),
        }
        match verify(&eval_claim("x+1")) {
            ClaimVerdict::Fail { reason, .. } => assert!(reason.contains("未知变量"), "{reason}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn eval_non_finite_results_fail_instead_of_passing() {
        match verify(&MathClaim::Eval {
            expr: "1/0".into(),
            expected: None,
            tolerance: None,
        }) {
            ClaimVerdict::Fail { reason, computed, .. } => {
                assert!(reason.contains("非有限"), "{reason}");
                assert_eq!(computed, None);
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn eval_rejects_invalid_tolerance() {
        match verify(&MathClaim::Eval {
            expr: "1+1".into(),
            expected: Some(2.0),
            tolerance: Some(-1.0),
        }) {
            ClaimVerdict::Fail { reason, .. } => assert!(reason.contains("tolerance"), "{reason}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    // ── claim 核验语义：identity ────────────────────────────────────────

    #[test]
    fn identity_passes_for_true_identities() {
        assert_eq!(verify(&identity_claim("x^2", "x*x", &["x"])), pass(None));
        assert_eq!(
            verify(&identity_claim("(x+y)^2", "x^2+2*x*y+y^2", &["x", "y"])),
            pass(None)
        );
        assert_eq!(
            verify(&identity_claim("sin(x)^2+cos(x)^2", "1", &["x"])),
            pass(None)
        );
        // 无变量 = 常量恒等（每点同值）。
        assert_eq!(verify(&identity_claim("2^3", "8", &[])), pass(None));
    }

    #[test]
    fn identity_failure_carries_counterexample() {
        match verify(&identity_claim("x^2", "(x+1)^2", &["x"])) {
            ClaimVerdict::Fail {
                reason,
                counterexample,
                ..
            } => {
                assert!(reason.contains("采样核验失败"), "{reason}");
                let example = counterexample.expect("counterexample present");
                assert!(example.contains("x="), "{example}");
                assert!(example.contains("lhs=") && example.contains("rhs="), "{example}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn identity_is_deterministic() {
        // A2：固定种子 LCG——同 claim 两次核验逐字节同 verdict（真机可复跑
        // 对账的前提）。
        let first = verify(&identity_claim("x^2", "(x+1)^2", &["x"]));
        let second = verify(&identity_claim("x^2", "(x+1)^2", &["x"]));
        assert_eq!(first, second);
        let json = serde_json::to_string(&first).expect("serialize");
        let again = serde_json::to_string(&second).expect("serialize");
        assert_eq!(json, again);
    }

    #[test]
    fn identity_low_explicit_sample_counts_verify_normally() {
        // w1 评审修复红绿样：wire 契约允许模型显式传 samples:1|2——恒等式
        // 成立时按实际点数核验必须 Pass（旧 `valid < 3` 下限把这些合法低点
        // 数核验误判成 Fail「两侧非有限」，点实为有限且相等）。
        for samples in [1u8, 2] {
            let claim = MathClaim::Identity {
                lhs: "x^2".into(),
                rhs: "x*x".into(),
                variables: vec!["x".into()],
                tolerance: None,
                samples: Some(samples),
            };
            assert_eq!(verify(&claim), pass(None), "samples={samples} must pass");
        }
        // 低点数不豁免核验本身：恒错等式在单点采样下照样红（确定性 LCG 钉住）。
        let wrong = MathClaim::Identity {
            lhs: "x".into(),
            rhs: "x+1".into(),
            variables: vec!["x".into()],
            tolerance: None,
            samples: Some(1),
        };
        assert!(
            matches!(verify(&wrong), ClaimVerdict::Fail { .. }),
            "samples=1 恒错等式仍必红"
        );
    }

    #[test]
    fn identity_skips_singularities_and_reports_when_no_valid_points() {
        // ln(-1) 在每个采样点都非有限：补采上限耗尽后如实报「无有效采样点」，
        // 文案只报真实非有限计数（缺省 7 点 × 补采上限 3 = 21 次全非有限）。
        match verify(&identity_claim("ln(-1)", "0", &["x"])) {
            ClaimVerdict::Fail { reason, .. } => {
                assert!(reason.contains("无有效采样点"), "{reason}");
                assert!(
                    reason.contains("21 个采样点两侧非有限"),
                    "文案须报真实非有限计数：{reason}"
                );
            }
            other => panic!("expected Fail, got {other:?}"),
        }
        // 奇点混合形态：1/x 在部分点有限——恒等式自身成立即 Pass（跳过点
        // 不计入失败）。
        assert_eq!(
            verify(&identity_claim("1/x + 1", "1 + 1/x", &["x"])),
            pass(None)
        );
    }

    #[test]
    fn identity_respects_sample_count_and_dedup() {
        // samples 钉点数：恒错等式在最小点数下也必红。
        let claim = MathClaim::Identity {
            lhs: "x".into(),
            rhs: "x+1".into(),
            variables: vec!["x".into(), "x".into()],
            tolerance: None,
            samples: Some(5),
        };
        match verify(&claim) {
            ClaimVerdict::Fail { counterexample, .. } => {
                assert!(counterexample.is_some());
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn identity_expression_parse_errors_are_fail_verdicts() {
        match verify(&identity_claim("2x", "2*x", &["x"])) {
            ClaimVerdict::Fail { reason, .. } => {
                assert!(reason.contains("隐式乘法"), "{reason}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    // ── serde 契约（wire 形状 = 工具 op 的严格入参） ────────────────────

    #[test]
    fn claim_json_round_trip_both_kinds() {
        let eval: MathClaim = serde_json::from_value(serde_json::json!({
            "kind": "eval", "expr": "1+1", "expected": 2.0
        }))
        .expect("eval claim parses");
        assert_eq!(
            eval,
            MathClaim::Eval {
                expr: "1+1".into(),
                expected: Some(2.0),
                tolerance: None,
            }
        );
        let identity: MathClaim = serde_json::from_value(serde_json::json!({
            "kind": "identity", "lhs": "x^2", "rhs": "x*x", "variables": ["x"], "samples": 9
        }))
        .expect("identity claim parses");
        assert_eq!(
            identity,
            MathClaim::Identity {
                lhs: "x^2".into(),
                rhs: "x*x".into(),
                variables: vec!["x".into()],
                tolerance: None,
                samples: Some(9),
            }
        );
    }

    #[test]
    fn claim_json_tolerates_unknown_fields() {
        // w1 评审修复红绿样：deny_unknown_fields 移除——全仓工具 op 参数均
        // 容多余字段，模型多带一字段不该吃 Err 多耗一轮自纠。
        let eval: MathClaim = serde_json::from_value(serde_json::json!({
            "kind": "eval", "expr": "1", "wat": 1
        }))
        .expect("eval claim tolerates unknown fields");
        assert_eq!(eval, eval_claim("1"));
        let identity: MathClaim = serde_json::from_value(serde_json::json!({
            "kind": "identity", "lhs": "x", "rhs": "x", "variables": [], "extra": true
        }))
        .expect("identity claim tolerates unknown fields");
        assert_eq!(identity, identity_claim("x", "x", &[]));
    }

    #[test]
    fn claim_json_rejects_unknown_kind() {
        assert!(
            serde_json::from_value::<MathClaim>(serde_json::json!({ "kind": "equation", "expr": "1" }))
                .is_err(),
            "未支持 kind 如实报错（w3 才有 equation）"
        );
    }

    #[test]
    fn verdict_serializes_with_kind_tags() {
        let pass_json = serde_json::to_value(pass(Some(2.0))).expect("serialize");
        assert_eq!(pass_json["kind"], "pass");
        assert_eq!(pass_json["value"], 2.0);
        let fail_json = serde_json::to_value(fail("boom")).expect("serialize");
        assert_eq!(fail_json["kind"], "fail");
        assert_eq!(fail_json["reason"], "boom");
    }
}
