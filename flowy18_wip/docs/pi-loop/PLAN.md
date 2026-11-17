# pi-rust 最小loop实验

执行范围来自用户已确认的一工作日实验；优先正确性与证据。新会话先读PROGRESS.md。拿不准写BLOCKED.md继续独立工作。

## 决策
- 固定git SHA 300c1f3b560b653b812a8877956dc1a1776629b7，依赖rpi-agent/rpi-ai同源同rev。
- 本期不fork、不迁移生产，不建立多内核抽象。使用脚本化provider测试loop，不能用假loop代替被测库。
- 单独workspace，避免修改生产锁文件。Windows先实测，其他平台不冒称通过。

## 边界
只改experiments/pi-loop/及其测试/记录。允许独立Cargo依赖及锁文件。现有生产代码、旧测试、locale、主workspace/Cargo.lock只读。不commit/push。不清理既有未跟踪文件。

## 任务0
基线e5524a21b9a1303d0e3b6f8918654a4ab1925204，分支experiment/pi-loop-minimal。cargo版本与依赖解析实测后记录。

## 实验
1. 独立crate依赖解析，保存metadata/tree和实际编译结果；不得带rpi-tools/harness/durable/cli/tui/runtime/extensions。
2. 离线文本增量/唯一终态、工具结果续跑、取消及晚到事件、同历史模型A→B、已有流事件转换五类行为测试。
3. 一个关键映射变异红→绿验证；报告传递依赖和薄适配成本，不修改断言掩盖失败。
4. Cargo串行；同命令连败3次换独立项。环境失败不算功能通过。

## 完成
以实际输出判断可直接接入/需小补丁/暂不值得；证据可复跑。没有完成的项目与真实云端/跨平台限制明确记录。实验仅做验证，不自动进入生产迁移。
