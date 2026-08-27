# Plan3 测试与 E2E 验收

## 1. 测试层级

| 层级 | 重点 |
|---|---|
| 参考行为差分 Fixture | Message/Event/Tool/Compaction 与固定 `earendil-works/pi` Commit 的选择性行为矩阵一致 |
| Rust Core 单元 | Message、Context Projector、Loop、Operation State、Compaction、Tool、Cancel |
| Rust Runtime 集成 | Compiler、Bundle、Adapter、Ledger、CAS、Lease、Trace、Recovery |
| Contract | Definition/Manifest/Bundle/Core State/Trace Schema 与 Rust serde round trip |
| Runtime Slice | MySQL+Redis+OSS、Worker、Provider/OpenSandbox Fixture 的真实状态断言 |
| Web | Inspector、Resource Picker、Canvas、Serializer、Trace/Session UI |
| Kubernetes E2E | 完整发布、调用、故障、安全、多副本和清理 |

不得用 Mock Agent Terminal 代替 Agent Core，不得用内存 Session 代替 Runtime 权威 Entry/Register/Usage，不得用宿主临时目录代替 OpenSandbox。

## 2. 临时 Kubernetes 规范

- 每次 Run 使用唯一 Namespace，例如 `agentx-p3-e2e-<run-id>`。
- 可按全局规范暂时缩容开发 `agentx` Runtime 服务，但必须记录原副本数并在 `finally` 恢复。
- 通过 Helm/agentxctl 安装 Control、Runtime、Observability、Dependencies 和内嵌 Agent Core 的 Worker Candidate。
- UI 使用 1440×900 Chromium，通过可见交互创建 Workflow、选择 Model、连接 Attachment、发布并调用。
- API/数据库只用于环境准备、故障注入和结果断言，不得绕过 UI 证明 Studio 能力。
- 成功或失败均收集 Pod/Events/Logs/JUnit/Trace/DB 快照，随后删除临时 Namespace。
- 证据禁止保存 Credential、完整 Prompt、长期记忆正文或用户敏感消息。

## 3. 产品 E2E 场景

| ID | 场景 | 自动化断言 |
|---|---|---|
| P3-E2E-001 | Agent 内置 Model | Inspector 选择并授权 Model；画布无 Model Port；Bundle 固定 Revision；Agent 成功执行 |
| P3-E2E-002 | 独立 Model Node | Model Node 仍完成一次调用；不能连接 Agent；结果/成本契约不回归 |
| P3-E2E-003 | Agent Core Loop | 模型请求 Tool、Tool Result 返回、模型继续并终止；Turn/Message/Effect 顺序与冻结行为矩阵一致 |
| P3-E2E-004 | 可选 Workspace Sandbox 与四工具 | 未选择 Sandbox 时模型 Tool Registry 无 read/write/edit/bash 且不创建 Lease；选择后四工具在 Agent Workspace 执行、Artifact 可下载、宿主路径不可见 |
| P3-E2E-005 | HTTP/SSE/stdio 多 MCP | stdio Server 创建时必选自己的 Sandbox；无 Agent Sandbox 的 Agent 仍可调用 stdio MCP；同一 Agent 调用多个 Transport/Server，未绑定 Tool 不出现 |
| P3-E2E-006 | Skill 与 Knowledge | Skill 指令/资产加载，Knowledge 检索带 Citation；Control 离线仍可运行 |
| P3-E2E-007 | Session Policy | `application_session` 第二轮使用第一轮消息和工具状态，另一个 Agent Node 不共享；`invocation` 下一次 Execution 不加载历史；持久模式缺 Session ID 明确失败 |
| P3-E2E-008 | 自动压缩 | threshold 与 overflow 生成 Summary/retained tail 后继续；完整 Entry 历史和 Usage 可查 |
| P3-E2E-009 | 跨 Session 长期记忆 | 同可信 Subject 新 Session 可召回；关闭/无授权/无 Subject 时不召回 |
| P3-E2E-010 | 恢复和幂等 | 在 Model/Tool/Compaction Intent、Effect、Settlement、Terminal 各边界强退 Worker/Sandbox，按 Replay Policy 收敛唯一终态 |
| P3-E2E-011 | 多租户安全 | 伪造 external ID、跨 App/Tenant Memory、Workspace、Object Handle 和 Tool ID 全拒绝 |
| P3-E2E-012 | Studio/Trace/诊断 | 中英文、浅深主题、键盘、授权六态、Agent Run/Turn/Tool/Compaction 下钻可用 |

## 4. 故障矩阵

| 故障点 | 注入时机 | 预期 |
|---|---|---|
| Workflow Worker | LLM streaming / Tool Effect / Compaction / Settlement | 新 Worker 从完整 Operation State 恢复；safe 可重放，never 不盲目重放 |
| Fencing 竞争 | Effect sent / result received / settlement | 新 Worker 取得新 Fencing Token，旧 Worker 的 Settlement 拒绝 |
| Sandbox | write/edit/bash 中途 | Lease/Reaper 收敛，partial/unknown 语义正确，宿主无残留 |
| stdio MCP Process Sandbox | initialize / tools/list / tool call / stdout frame / terminate | 进程 Lease 收敛，stderr 不污染协议，飞行中未知结果稳定，Agent Workspace 不可见 |
| Runtime MySQL | State CAS / Tool Ledger | 不确认未持久事件；恢复后唯一推进 |
| Runtime Redis | Task pending / Agent running | MySQL Outbox 重建，Agent Session 不丢失 |
| Runtime OSS | State load / State save / Artifact | Hash/大小校验；不提交无对象引用的新版本 |
| Vault | Model/MCP Handle 获取 | Fail closed，无 Secret 泄露或 Control fallback |
| Egress Gateway | Model/MCP/Memory/Knowledge | 稳定错误、重试策略和 Trace，Sandbox 内网边界不放宽 |
| ClickHouse | Agent 运行全程 | Execution 成功，恢复后 Trace 补齐且不重复 |
| Control 全部服务/DB/OSS | 已激活 Bundle 调用 | Gateway→Worker/Agent Core→Tool→Output 完整成功 |

## 5. 安全矩阵

- `agentx-agent-core` Cargo 图不包含 SQLx、Redis、Reqwest、OSS/Vault/Kubernetes 或宿主文件 API；基础设施只有 Worker Adapter 可访问。
- Worker NetworkPolicy 与 Credential 权限不因 Agent Core 放宽；Core State/Trace 不含 Provider Secret 或 Sandbox Endpoint。
- read/write/edit/bash 拒绝绝对路径、`..`、软链接、硬链接/挂载逃逸、设备文件和跨 Workspace Handle。
- Agent Workspace Sandbox 与 stdio MCP Runtime Sandbox 使用不同 Binding/Lease/Workspace/Credential/NetworkPolicy；任何一方不能访问另一方 Handle。
- stdio MCP 禁止宿主进程、Shell command 字符串、动态包安装和无界 stdout/stderr；只从固定 Sandbox Profile 镜像启动结构化 command/args。
- Tool/MCP/Memory/Knowledge 结果作为不可信数据进入模型，不得改变 System/Grant/Tool Registry。
- Skill 只从 Bundle 签名对象加载，禁止 `.pi`、用户 Home、npm/git 动态安装、Pi CLI 和 Extension 自动发现。
- Agent State/Trace/Artifact 下载按 Execution/Session/Tenant 授权并脱敏。
- Long-term Memory 只接受可信 Subject，删除/清除操作有审计和并发控制。
- 参考源码归因、Rust 依赖、镜像、SBOM、License 和签名符合供应链门禁；Worker 镜像不新增 Node/npm/Pi 包。

## 6. 多副本和容量

至少验证：

- `workflow-worker` 为 2 副本，两个副本均运行同一 Agent Core Contract Version。
- 运行中执行 Worker `2→4→2`，没有进程内 Session/Core 状态粘性。
- PDB Eviction、Rolling Upgrade 和 Drain 不丢事件。
- 并发 Agent Session、Stateless Run、Model Action、Agent Sandbox Tool、stdio MCP Process Session 和 Compaction 各有容量阈值。
- 冻结队列等待、Run 延时、事件背压、State Object 大小、Compaction 时延和外部 Provider 并发指标。
- 两小时稳定性结束后：活跃 Lease、未确认事件、未发布 Outbox、孤儿 Sandbox、未引用 State Object 和用量漂移均为 0。

## 7. 静态与快速门禁

- Rust fmt、Clippy `-D warnings`、Workspace/Test/Doc Test、Core 状态机与参考差分 Fixture。
- Core Cargo 依赖边界扫描，禁止基础设施客户端和宿主文件 API。
- Web Oxlint、Vitest、TypeScript、Vite build。
- JSON Schema/Proto/OpenAPI/生成类型无漂移。
- Control/Runtime/Observability 空库 Migration。
- Helm lint/template、Values Schema、NetworkPolicy 和资源所有权。
- 2000 行限制、依赖边界、Secret/Env/SQL 静态扫描。
- `rg` 证明旧 `ai_model` Slot、旧临时 Loop、远程 Pi Runtime/协议、fallback 和双模配置已删除。
- `git diff --check`。

## 8. 证据结构

```text
docs/plan3/evidence/
  p3-00.md ... p3-07.md

artifacts/plan3/<run-id>/
  junit/
  web/
  contracts/
  mysql/
  redis/
  trace/
  kubernetes/
  security/
  capacity/
  cleanup/
```

阶段证据必须记录：Git Commit、参考 Pi Commit/包版本、行为矩阵版本、镜像 Digest、Schema Hash、Kubernetes 版本/CNI/RuntimeClass、测试命令、开始结束时间、结果、已知限制和清理结果。
