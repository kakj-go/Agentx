# P3A-002 Pi 固定参考审计

状态：`done`。这是工程审计，不构成法律意见。

## 1. 固定来源与校验

| 项 | 固定值 |
|---|---|
| Repository | `https://github.com/earendil-works/pi` |
| Commit | `a69bef789bc95abf0acee16f7b4660b70b650bb9` |
| Git tree | `b425b8f46d9128db9fa9128e87fd7a5e67becf4d` |
| Package | `@earendil-works/pi-agent-core@0.84.2` |
| Package engine | Node `>=22.19.0`（只用于上游审计，不进入 Agentx） |
| License | MIT，Copyright (c) 2025 Mario Zechner |

`scripts/plan3/verify_pi_reference.py` 会校验 Commit、包版本、License、15 个 Fixture 的版本绑定，以及以下关键源码 Git Blob：

| 上游文件 | Blob |
|---|---|
| `packages/agent/src/agent-loop.ts` | `a251fede0a9adb9c6cf5e2ba57b4ea2f65b8100b` |
| `packages/agent/src/agent.ts` | `0de7edd83029743e59a174c8b9b994282fd5f8a3` |
| `packages/agent/src/harness/compaction/compaction.ts` | `06ae8afb1dd0dc21215fef60ed8266e5a48649e6` |
| `packages/agent/src/harness/session/context.ts` | `d219b541ae1fbb74ac64e07201bc9d062972ea55` |
| `read/write/edit/bash.ts` | `5fdbdf6...` / `f717528...` / `5473c48...` / `c0e1f19...` |

License 结论：P3-00 没有把上游 TypeScript 源码复制进 Agentx；Rust 实现只对齐公开可观察行为。文档保留项目、作者和许可证归因。如果未来复制上游源码或实质性源码片段，必须随复制内容保留完整 MIT Notice。

## 2. 成熟度与行为决策矩阵

| 能力 | 上游证据 | 成熟度 | 决策 | Agentx 差异 |
|---|---|---|---|---|
| Agent loop / Tool loop | `agent-loop.ts`、`agent-loop.test.ts` | 已实现且有测试 | adopt | Rust 状态机；Effect 经 Port/持久 Ledger |
| AgentMessage→LLM Context | `types.ts`、`agent-loop.ts::convertToLlm` | 已实现 | adopt | 存储 Entry 与 Context Projection严格分离 |
| steering/follow-up | `agent.ts` queues、loop polling、tests | 已实现 | adopt | 队列必须 durable；steer 先于 follow-up |
| Tool 顺序/并行、错误回填 | `executeToolCalls*`、tests | 已实现 | adapt | 首个垂直切片顺序执行；P3T-005 冻结批计划 |
| threshold Compaction | `harness/compaction/compaction.ts` | 算法和测试已实现 | adapt | 以完整 Operation State + 独立 Usage Effect 实现 |
| overflow Compaction | Pi AI overflow tests + Harness compaction | 组成能力存在 | adapt | Provider overflow 触发一次压缩并用稳定操作恢复 |
| Context Projection | `harness/session/context.ts` | 已实现且有测试 | adopt | 最新 Summary + retained tail + recent；不兼容 Pi Entry 格式 |
| Session storage | Harness session memory/JSONL + conformance | 实现成熟度不一 | adapt | Agentx 使用 Runtime MySQL/OSS Entry/Register/Usage，不引入 JSONL/SQLite |
| read/write/edit/bash | Harness tool implementations/tests | 已实现且测试丰富 | adapt | Schema/截断语义参考；效果只能在 OpenSandbox Port 中执行 |
| AgentHarness | `agent-harness.ts`、`agent-harness-scaffold.test.ts` | 明确 scaffold；prompt/compact/resume/queue/watch 等抛 `HarnessNotImplemented` | exclude | 不把 scaffold 当完成设计；只采用已实现的独立 Session/Compaction/Tool 行为 |
| 多 Lane/Tree Navigation/Branch Summary | Harness 类型与部分 Session 代码 | 非首期目标 | exclude | P3-00/首期只使用 `main` Lane |
| Pi CLI/npm Runtime/Node env | coding-agent/package scripts | 已实现但架构不适用 | exclude | 不进入 Worker 镜像，不新增部署单元 |
| Pi JSONL/SQLite 格式 | Harness/session backends | 已实现但存储边界不适用 | exclude | 无兼容、migration 或 fallback |

## 3. 可重复验证

快速固定校验：

```bash
uv run --frozen python scripts/plan3/verify_pi_reference.py
cargo test -p agentx-agent-core
```

如需重跑跨平台上游测试，使用 `--run-upstream-tests`；它覆盖 Loop、Compaction、Session Context、read/write/edit。Bash 上游测试需要系统中存在可用的 `/bin/bash`，使用 `--run-upstream-tests --run-bash-tests` 显式开启。npm 依赖只安装在系统临时 Pi checkout，不写 Agentx 仓库或生产镜像。Fixture 的规范化和更新规则见 `fixtures/agent-core/README.md`。
