# P3A-004～006 冻结架构决策

以下 ADR 状态均为 `accepted`，是 P3-01 的输入，不代表已接生产路径。

## ADR-P3-001：内嵌 Rust Agent Core 与 Port 边界

- 在现有 Rust Workflow Worker 进程内使用独立 crate `agentx-agent-core`；不新增 Node/TypeScript Runtime、gRPC、服务或镜像。
- Core 只依赖纯领域类型和 `ModelPort/ToolPort/StatePort/EventPort/ClockPort/BudgetPort`。
- SQL、Redis、HTTP、OSS、Vault、Kubernetes、OpenSandbox Endpoint、Credential 和宿主文件/进程 API 都只能存在于 Worker Adapter。
- Model 是 Agent Definition 内部必选精确版本 Reference。MCP、Skill、Knowledge、Long-term Memory由 Workflow Attachment 解析后作为授权能力注入。
- P3-00 crate 不被生产 Worker 依赖；P3-02 才实现 Adapter。

## ADR-P3-002：完整 Operation State 与效果恢复

```text
accepted/checkpoint
  → intent_persisted
  → effect_started
  → effect_completed
  → settled_success | settled_failure
  → next_checkpoint | terminal
                    ↘ unknown_outcome（暂停，禁止盲重放）
```

- 每个 Model、Tool、Compaction Effect 都保存稳定 `operation_id/effect_id/attempt/intent/effect_identity/replay_policy/fencing_token/timestamps`。
- `IntentPersisted + ledger not_started` 使用同一 ID执行；已确认结果只补 Settlement。
- `EffectStarted` 后只有 `safe + ledger not_started` 可以同 ID重试；`never`、ledger unknown 或无法对账一律进入 `unknown_outcome`。
- Settlement 存在时直接恢复后续状态，不能重复 Effect。
- Session commit 使用 Expected Version + Fencing Token；同一持久 Session/stable Node Key只允许一个开放 Operation。

## ADR-P3-003：两类 Sandbox 与 stdio Process Session

- Agent Workspace Sandbox 是 Agent Inspector 内部可选 `0..1` Reference。未选择时 Tool Registry不注册 `read/write/edit/bash`；选择后四工具只能经 `ToolPort → OpenSandbox Adapter`。
- stdio MCP Server Version创建/更新时 Runtime Sandbox必选 `1`；HTTP/SSE 不要求该字段。
- Tool 自动继承 Server Version 与 Runtime Sandbox Version，不允许逐 Tool覆盖。
- 两类 Sandbox的 Binding、Lease、Workspace、Credential、NetworkPolicy、TTL 和 Reaper完全独立；Agent 无 Workspace Sandbox时仍可使用已授权的 stdio MCP。
- P3-00 只冻结 `SandboxProcessSessionContractV1`：start、bounded stdin/stdout JSON-RPC frame、独立 stderr、wait、interrupt、terminate、heartbeat、timeout、lease/fencing、reconcile/cleanup；生产实现归 P3C-002。

## ADR-P3-004：Worker 并发、背压、升级和容量

P3-02 初始每 Worker硬上限：活跃 Agent Run 64、并发 Model Effect 16、Agent Sandbox Effect 16、Compaction 4、stdio MCP Process Session 8。到达上限时新 Invocation留在 Runtime 队列；持久 Session队列最多 32 条，超出返回 `AGENT_SESSION_BUSY`。

- Core/Session 无进程粘性；每次推进前取得 Lease并验证 Fencing，Worker 副本可 `2→4→2`。
- 单 Core State序列化上限 1 MiB；更大 Prompt/Result/Summary 使用 Runtime Object Reference。单 Run事件缓冲 1024 条，满后暂停读取 Provider/Sandbox流形成背压，不丢终态。
- Model/Tool Stream按 64 KiB 或 250 ms 聚合 Trace delta；Token delta不逐条写 ClickHouse Span。
- Worker 收到 Drain后拒绝新 Claim，给当前 Effect 120 秒完成；超时持久化可恢复状态后释放 Lease。滚动升级仅允许同一个 Core Contract major；不匹配返回 `AGENT_CORE_VERSION_MISMATCH`。
- 生产认证指标：队列等待、Run 延时、各 Effect并发、State大小、Compaction时延、Event buffer使用率、Lease lost、Outcome Unknown、孤儿 Sandbox/Process、未 Settlement Effect。
- P3X-005 必须用双副本、扩缩容、PDB Eviction、Rolling Upgrade 和两小时稳定性校准以上初始上限；改变上限不改变契约或恢复语义。

## ADR-P3-005：契约版本与破坏性替换

- 冻结 Definition 6.0、Manifest 2.0、Bundle 2.0、Agent Core Contract 1.0、AgentRunInputV1、Session/Operation V1 和 Process Session V1。
- P3-01 直接删除 Agent v1 Slot/DTO，不加兼容层、migration 或 fallback；开发阶段历史草稿可清理。
- `contracts/schemas/agent-core-v1` 和 `contracts/openapi/agent-core-contracts-v1.json` 由 Rust 类型生成，`schema_drift` 测试防止漂移。
