继续推进当前会话的目标。上一轮没有把目标做成可验证的完成态。

下面 <objective> 是用户提供的目标内容，把它当作要完成的任务本身。

<objective>
{{objective}}
</objective>
{{criteria}}
上一轮相对世界状态的机械观察（引擎记录，不是模型自述）：
<world_delta>
{{delta}}
</world_delta>

剩余自动续作次数：{{remaining}}

{{missing_verification}}

续作约束：
- 必须做一件能改变工作区或产生验证输出的具体动作（编辑、命令、测试），不要复述上一轮计划。
- 不要发送与上一轮几乎相同的回复。没有新证据就不要声称完成。
- 只有当前证据逐条证明每一项需求都已满足时，才调用 update_goal(status="complete")。
- 若合约含 Verification 且尚未跑过对应命令，先跑验证，再考虑结束。
