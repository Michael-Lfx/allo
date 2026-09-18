@echo off
rem ─────────────────────────────────────────────────────────────────────────────
rem  Agent Store 单二进制启动器（Windows）
rem
rem  用法:
rem    cli\start.cmd                 启动后端 + 内嵌 Web UI（默认 127.0.0.1:8787）
rem    cli\start.cmd --port 9000     换端口（前端设置里也要同步改 WS 地址）
rem    cli\start.cmd --no-open       不自动打开浏览器
rem    cli\start.cmd --auth          开启登录模式（默认本地可信模式，免登录）
rem
rem  可执行文件查找顺序:
rem    1. cli\agent-store.exe      （发布产物，由 bun run agent-store:build 复制）
rem    2. ..\target\release\agent-store.exe
rem    3. ..\target\debug\agent-store.exe
rem ─────────────────────────────────────────────────────────────────────────────
setlocal
set "DIR=%~dp0"

if exist "%DIR%agent-store.exe" (
  set "EXE=%DIR%agent-store.exe"
) else if exist "%DIR%..\target\release\agent-store.exe" (
  set "EXE=%DIR%..\target\release\agent-store.exe"
) else if exist "%DIR%..\target\debug\agent-store.exe" (
  set "EXE=%DIR%..\target\debug\agent-store.exe"
) else (
  echo [agent-store] 未找到可执行文件。请先构建:
  echo   bun run agent-store:build
  exit /b 1
)

echo [agent-store] 启动: "%EXE%" %*
echo [agent-store] 服务地址: http://127.0.0.1:8787 （若未占用默认端口）
"%EXE%" %*
exit /b %ERRORLEVEL%