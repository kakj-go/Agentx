# P3-01 决策完备输入

P3-01 可以直接实施 Definition、Manifest、Compiler 和 Studio，不得重新打开下列决策：

- Model 是 Inspector 内必选精确版本 Reference；删除 Model Canvas Attachment/Port。
- Workspace Sandbox 是 Inspector 内可选 `0..1` Reference；未选时四工具完全不可见。
- MCP/Skill/Knowledge/Long-term Memory保持 Canvas Attachment。
- Session Policy必须显式选择 `application_session|invocation`，不按用户 ID猜测。
- Definition/Manifest/Bundle直接升级到 6.0/2.0/2.0，不保留 Agent v1兼容。
- Bundle按 `frozen-contracts.md` 计算完整资源和 Grant闭包，两类 Sandbox不互相满足依赖。
- P3-01 只改变设计/编译/发布契约；生产 Worker仍走旧 `execute_agent`，直到 P3-02 Adapter和 P3-06切换。

实施输入：机器 Schema、Rust契约类型、`deletion-ledger.json`、Fixture、Core Port、错误码及 `architecture-decisions.md`。P3-01完成后重新生成 Workflow Schema、前端类型和测试 Fixture，并关闭台账中的 `replace-p3-01` 项。
