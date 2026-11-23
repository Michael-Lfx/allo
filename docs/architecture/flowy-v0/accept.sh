#!/usr/bin/env bash
# 本地受理回执 · 端到端验收
#
# 证明三件事：
#   1. 忙时提交会被接住，拿到一个持久化的回执编号（走真实 UI 代码路径）
#   2. 换一个队列句柄 / 客户端重开，回执还在
#   3. 应用能用 bash 后台启动（不需要点任何确认）
#
# 用法：
#   bash docs/architecture/flowy-v0/accept.sh              # 契约测试（默认，快）
#   WITH_APP=1 bash docs/architecture/flowy-v0/accept.sh   # 额外验证启动链
#
# 退出码：0 全过；非 0 表示有断言失败。

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
WS="$REPO/flowy18_wip"
EXE="$WS/target/debug/flowy-app.exe"

pass=0
fail=0

ok()   { printf '  [PASS] %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  [FAIL] %s\n' "$1"; fail=$((fail+1)); }
head() { printf '\n== %s ==\n' "$1"; }

cd "$WS" || { echo "找不到 flowy18_wip 工作区：$WS"; exit 2; }

# ---------------------------------------------------------------------------
head "1/4 存储契约：受理后可被新句柄读到"
# ---------------------------------------------------------------------------
if cargo test -q -p flowy-agent-core --lib acceptance 2>&1 | grep -q "0 failed"; then
    ok "acceptance 日志测试全过（含「只增不改」「尾部截断容忍」）"
else
    bad "acceptance 队列测试失败"
    cargo test -p flowy-agent-core --lib acceptance 2>&1 | tail -20
fi

# ---------------------------------------------------------------------------
head "2/4 agent 契约：忙时受理返回回执，另一个 handle 读得回"
# ---------------------------------------------------------------------------
if cargo test -q -p flowy-agent --lib accept_deferred 2>&1 | grep -q "0 failed"; then
    ok "accept_deferred：受理拿到 receipt，独立 handle 重新读到同一条"
else
    bad "accept_deferred 测试失败"
    cargo test -p flowy-agent --lib accept_deferred 2>&1 | tail -20
fi

# ---------------------------------------------------------------------------
head "3/4 UI 端到端：运行中调用 send_prompt，落盘且重开可读"
# ---------------------------------------------------------------------------
# 走的是真实的 StudioPanel::send_prompt，不是另写的模拟路径。
if cargo test -q -p flowy-studio --lib sending_while_running 2>&1 | grep -q "0 failed"; then
    ok "运行中提交被受理：队列多一条、草稿清空、换句柄仍读得到"
else
    bad "UI 端到端测试失败"
    cargo test -p flowy-studio --lib sending_while_running 2>&1 | tail -25
fi

# ---------------------------------------------------------------------------
head "4/4 启动链（可选）"
# ---------------------------------------------------------------------------
if [[ "${WITH_APP:-0}" == "1" ]]; then
    if [[ ! -x "$EXE" ]]; then
        bad "二进制不存在：$EXE"
    else
        # 只允许 bash 后台启动；禁止 PowerShell Start-Process（会弹签名确认）
        taskkill /F /IM flowy-app.exe >/dev/null 2>&1
        ( "$EXE" >/tmp/flowy-accept.log 2>&1 & )
        sleep 10
        # 注意：这个环境没有 pgrep，用 ps 计数（[f] 防止 grep 匹配自己）
        alive="$(ps aux 2>/dev/null | grep -c "[f]lowy-app")"
        if [[ "${alive:-0}" -gt 0 ]]; then
            ok "bash 后台启动成功，进程存活（无人工点击）"
        else
            bad "进程未存活，见 /tmp/flowy-accept.log"
        fi
        printf '  回执落盘位置：%s\n' "$HOME/AppData/Roaming/Flowy/Flowy/data/acceptance/journal.jsonl"
        taskkill /F /IM flowy-app.exe >/dev/null 2>&1
    fi
else
    printf '  跳过（设置 WITH_APP=1 开启）\n'
fi

# ---------------------------------------------------------------------------
printf '\n== 结果 ==\n'
printf '  通过 %d，失败 %d\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]] && printf '  ✔ 受理纵线契约成立\n' || printf '  ✘ 有断言未通过\n'
exit $(( fail == 0 ? 0 : 1 ))
