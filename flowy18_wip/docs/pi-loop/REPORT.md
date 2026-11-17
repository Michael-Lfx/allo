# pi-rust 最小loop实验结果

## 结论
建议进入薄适配开发阶段，当前无需fork即可调用低层loop；不能直接迁移生产。固定SHA 300c1f3b560b653b812a8877956dc1a1776629b7，未修改上游源码，未使用高层Agent/harness。实验耗时在一天预算内；首次编译39.30s，不是严格基准测试。

## 实测
- cargo metadata --format-version 1 --manifest-path experiments/pi-loop/Cargo.toml：退出0，首次锁定177个packages。该数字包含目标平台/构建等解析包，不等于生产新增依赖数。
- cargo tree --manifest-path experiments/pi-loop/Cargo.toml --locked --offline -e normal：退出0，见dependency-tree.txt。上游crate仅rpi-agent/rpi-ai/rpi-telemetry；没有rpi-tools/harness/durable/cli/tui/runtime/extensions。jsonschema 0.18.3间接引入clap 4.6.7与reqwest 0.12.28。
- cargo test --manifest-path experiments/pi-loop/Cargo.toml --locked --offline：4 passed/0 failed/0 ignored，见final-test-output.txt。
- 测试覆盖真实run_agent_loop：文本增量和唯一结束；保留历史model-a→model-b；echo工具真实execute后call-1结果进入第二次provider请求；取消token抵达provider，Aborted终态及迟到文本抑制；现有StreamChunk序列化文本协议经薄桥接进入loop。
- 故意将桥接text.push_str(delta)改为broken-mapping，指定桥接测试0passed/1failed，退出101（mutation-red.txt）；恢复实现不改断言，全套4/4绿。
- git diff --stat无输出，生产已跟踪文件零修改。

## 边界和已知未知
这是脚本化provider的离线实验，不是真实HTTP/SSE或云模型兼容性验收。取消依赖provider遵守token并发送Aborted；本实验不能证明不合作provider会被强制中断。
桥接实现只支持一个文本块；不是生产全协议适配。reasoning/usage/工具参数增量/图像等明确拒绝，未验证原始工具参数字符串→JSON的无损转换；工具测试直接使用pi类型，不能据此声称旧工具协议全兼容。
模型切换是run之间的配置快照；不支持run中切换。消息记录由调用方保存，本实验仅验证内存历史传入，不验证持久化恢复。
Windows本机通过；其他OS、上游声明MSRV、二进制尺寸、网络故障与真实凭证均未验证。

## 最小接入建议
1. 固定当前SHA，保留独立低层loop入口，暂不fork。
2. 下一阶段只加flowy-agent内部适配模块：输入历史→pi context、现有流→pi event、pi event→现有投影、取消token联动；不引入rpi高层存储或工具集。
3. 先实现text/reasoning/usage/工具调用id和参数完整转换与错误终态；通过后再替换生产执行路径。现有历史格式/UI/账号目录保持不变。
4. 工具执行走现有宿主授权与调度，首版串行；设置run请求次数上限。未完成真实流转换与历史保真前不删除dsh旧路径。
5. 只有实测依赖预算不接受时才开小补丁fork裁剪jsonschema默认features，先证明参数校验不退化；不为看起来小而复制/拆散loop源码。

## 复现
在仓库根执行上述cargo test/tree，使用独立锁文件。运行会使用当前Cargo配置与缓存。原始实验计划见PLAN.md，进度见PROGRESS.md。
