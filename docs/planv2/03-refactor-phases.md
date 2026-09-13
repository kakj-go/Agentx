# V2 破坏性重构阶段计划

## 1. 总体策略

本次采用“新架构空库重建 + 一次性切换”，不采用旧库在线拆分：

- 删除当前共享 MySQL Migration 链，分别建立 Control/Runtime 初始 Schema。
- 不迁移开发数据，不实现 V1/V2 双写、双读或回填。
- 允许修改所有内部 Crate API、gRPC、HTTP、事件和配置变量。
- 每个功能切片完成替代链路和回归后立即删除该功能的旧路径；不得在替代链路尚未落地时提前删除仍被后续阶段使用的共享实现。
- V2 全链路完成前不宣称生产可用；阶段测试使用独立临时 Namespace。

每个阶段合入主线时必须保持：Workspace 编译、空库 Migration、静态边界扫描和该阶段回归测试通过。V2 不做生产双写或 V1 兼容开关，但允许在同一开发分支中用明确模块边界逐个完成垂直切片，避免“前半段已删除、后半段尚未实现”的长期不可运行状态。

## 2. V2-00：契约冻结和代码边界

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2A-001 | done | 冻结服务和数据所有权 | 服务运行契约、后台循环和端口清单 | 无未归属权威表或后台任务 |
| V2A-002 | done | 冻结 Bundle/Inbox/Outbox/Event 契约 | Rust DTO、JSON Schema、Internal OpenAPI | JCS/Hash/签名/未知字段/版本拒绝测试 |
| V2A-003 | done | 重组基础设施边界 | Control/Runtime Infrastructure Crate 和 Legacy facade | Runtime Cargo 路径不能依赖 Control Infrastructure |
| V2A-004 | done | 建立静态依赖门禁 | Cargo/SQL/Env/Secret/NetworkPolicy/行数检查 | 五类负向 Fixture 均非零退出 |
| V2A-005 | done | 删除兼容目标 | V1 删除台账和冻结 Legacy 基线 | 每项有 Owner、Replacement 和删除阶段 |
| V2A-006 | done | 冻结逐表处置目录 | 133 张表机器可读允许列表 | 无遗漏、重复或未定 Writer |
| V2A-007 | done | 冻结 Spec/Admission 和 Work Package | Bundle、Admission、Debug/Evaluation Package DTO | 签名覆盖不可变 Payload；Admission 独立于 Bundle |
| V2A-008 | done | 建立通用 Claim/Lease 基础库 | 数据库时间、Owner、Fencing、`SKIP LOCKED` Harness | 双面 Fixture、20 路 Claim、过期接管和旧 Token 拒绝 |
| V2A-009 | done | 冻结网络信任和服务运行契约 | 调用矩阵、端口、RS256/Delegation、容量 | Audience/Role/Scope/过期/租户/旧 Key 拒绝 |
| V2A-010 | done | 冻结 V2 版本支持窗口 | 版本矩阵和 Expand/Contract 规则 | 初始只接受 1；后续当前+上一版本 |

### 代码结构建议

```text
src/crates/
├── agentx-domain                    # 保留真正存储无关、跨面的值对象
├── agentx-application               # 迁移期保留纯 Port/Use Case，禁止导出 SQL 类型
├── agentx-runtime                   # 复用现有编译器、表达式和状态机内核
├── agentx-runtime-contracts         # 新增：Bundle、Work Package、Command/Event/API DTO
├── agentx-control-infrastructure
├── agentx-runtime-infrastructure
└── agentx-observability             # 只有出现共享 ClickHouse 查询模型时再提取
```

首轮只强制拆出版本化 Contracts、Control Infrastructure 和 Runtime Infrastructure，不一次性复制 Domain/Application 层。现有公共 Crate 通过依赖扫描逐步收窄；只有代码已经形成独立领域语言和发布节奏时，再拆 Control/Runtime Domain 或 Application。Repository、Settings、Migration 和 SQL Row 类型不能跨面导出，服务之间不能通过共享 Infrastructure Crate 绕过 Internal API。

### 退出条件

- 目标服务、数据、端口和中间件无开放决策。
- 编译期和 CI 能阻止 Runtime 依赖 Control Repository。
- Bundle 和跨面事件可以独立完成序列化、Hash 和重复应用测试。
- 原 `plan/99-feature-traceability.md` 中所有产品能力都有 V2 Owner、任务和回归测试，不允许用一个“语义等价”总项代替。

## 3. V2-01：分域基础设施和空库 Schema

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2D-001 | done | 建立 Control 初始 Migration | `deploy/migrations/control/0001_initial.sql` | 空库初始化、FK/索引/租户约束检查 |
| V2D-002 | done | 建立 Runtime 初始 Migration | `deploy/migrations/runtime/0001_initial.sql` | 空库初始化、状态机和 Outbox 约束检查 |
| V2D-003 | done | 建立 ClickHouse 初始 Migration | 独立 Migration 目录 | 空库创建、重复运行安全 |
| V2D-004 | done | 配置两个 MySQL 和独占 Runtime Redis | 独立环境变量、Secret、Doctor；Control 不部署 Redis | 交叉 Credential 访问失败；Control 不连接 Runtime Redis |
| V2D-005 | done | OSS 三域隔离 | Bucket/Prefix、IAM、Artifact Domain | 越域读写负向测试 |
| V2D-006 | done | 禁止新增共享 Schema 依赖 | 机器可读表允许列表和 SQL/Cargo/Env CI | 新代码越界立即失败；旧依赖有明确 Owner |
| V2D-007 | done | 建立最小部署隔离 | Namespace、ServiceAccount、Secret、NetworkPolicy 和独立 Migration Job 骨架 | Runtime 无 Control MySQL Secret；Control 无任何 Redis Secret；仅允许冻结的 Internal API |
| V2D-008 | done | 建立分域 Bootstrap/Fixture | Control/Runtime/Observability 空库初始化和基础测试身份 | 不用跨库 SQL Seed 业务对象 |
| V2D-009 | done | 清理 Control Redis 依赖 | 删除 Control Redis Settings、Secret、清单、客户端和 Key 协议 | Control 只使用 MySQL/Ingress/有界本地缓存；静态扫描无残留 |

### 退出条件

- 一个 Profile 能部署 2 MySQL、1 个独占 Runtime Redis、1 OSS、1 ClickHouse；不存在 Control Redis 配置、Secret 和清单。
- Control/Runtime Migration 可以分别执行、回滚应用但不回滚已执行 Schema。
- Runtime Pod 的环境变量中不存在 Control MySQL 地址和密码；仓库与清单中不存在 Control Redis 地址或凭据。
- Control Pod 的环境变量、Secret 和 NetworkPolicy 中不存在 Redis 依赖，也不能连接 Runtime Redis。
- Claim/Lease、独立数据库账号和最小跨面 NetworkPolicy 已可被后续所有阶段复用。

## 4. V2-02：Runtime Bundle 发布链路

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2P-001 | done | 发布时编译完整 Bundle | Bundle Builder | 相同输入产生相同 Hash |
| V2P-002 | done | 实现 Control Outbox/Publisher | `platform-control --role=publisher` | 多副本 Claim、崩溃恢复 |
| V2P-003 | done | 实现 Runtime Bundle Internal API | Prepare/Activate/Rollback/Disable | Inbox 幂等、签名/序列校验 |
| V2P-004 | done | 实现 Runtime 本地发布投影 | Bundle、Route、Head、Key、Trigger、授权表 | 生产查询不访问 Control DB |
| V2P-005 | done | 实现发布状态和 UI | Publish Attempt、失败原因、回滚 | UI/API 能识别 prepared/active/rejected |
| V2P-006 | done | 删除运行时控制数据拼装 | 删除 Coordinator 对 Version/Catalog/Grant 控制表读取 | 新 Execution 只接受 Bundle ID |
| V2P-007 | done | 复制 Runtime OSS 制品闭包 | Object Manifest、Content Hash、引用和补偿清理 | Runtime 断开 Control OSS 后仍可执行 |
| V2P-008 | done | 完成基础垂直切片 | 基础 Workflow 的 Gateway→Coordinator→Worker→Query 链路 | 控制面缩容/断网后连续执行 |
| V2P-009 | done | 实现 Bundle Reference/GC 基础骨架 | 通用 Reference/Hold 存储、`active_execution` 接入和对象级 Mark/Sweep | Active/Prepared/Head/Reference/Hold 阻止删除；孤儿对象 GC 幂等 |

### 退出条件

- 发布、重复发布、响应丢失、Publisher 崩溃和回滚测试全部通过。
- Bundle 激活后关闭控制面，基础 Application 从 Runtime Gateway 到最终输出和 Runtime Query 完整成功；不能只验证 Route/Workflow 能被解析。
- V2-02 时 Runtime Gateway 尚不持有 Redis 配置；11 张增量表全部登记唯一 Plane/Writer；Disable 和对象级 GC 的拒绝、并发及重试语义由自动化测试覆盖。V2-03 按冻结决策只为 SSE 唤醒引入 Runtime Redis，并由专项静态门禁限制模块边界。
- 可复现命令、最终 Kubernetes Run 和能力边界见 [V2-02 验收证据](evidence/v2-02.md)。

## 5. V2-03：Runtime Gateway 和生产业务状态

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2G-001 | done | 将 Application Route/API Key 移入 Runtime | Runtime Gateway Repository | Control DB 停机仍能鉴权和路由 |
| V2G-002 | done | 将 Session/Message/Invocation 移入 Runtime | Runtime Schema/API | 幂等、并发序列和终态测试 |
| V2G-003 | done | 建立 Runtime Query/SSE | Invocation/Execution 查询、Cursor SSE | 任意 Gateway Pod 可重连 |
| V2G-004 | done | 提取可独立启动的 Trigger Role | `workflow-runtime --role=trigger` 的 Schedule/Poll/Lifecycle；默认紧凑 Profile 合并运行 | 多副本无重复业务执行；不要求默认创建独立 Deployment |
| V2G-005 | done | 独立生产入口 | Runtime Ingress/域名 | Web/Platform API 为零副本仍可调用 |
| V2G-006 | done | 删除旧 Gateway 控制查询 | 删除 IAM/Department/Control Deployment SQL | Runtime 服务无 Control 表名 |
| V2G-007 | done | 完成 Gateway 模糊提交协议 | Invocation/Message/Command 原子事务和结果查询 | DB 已提交但响应丢失时幂等收敛 |
| V2G-008 | done | 冻结 Trigger 时间语义 | Timezone/Misfire/Catch-up/Cursor/禁用规则 | 时钟跳变、迟到和补偿不重复执行 |

### 退出条件

- API、Webhook、Schedule、Poll、SSE、Cancel 和 Wait Resume 完全使用 Runtime 依赖。
- Runtime MySQL 不可用时 Gateway 返回明确错误且不虚假接受请求。
- 最终 Kubernetes Run、自动化门禁和仍未完成的 V2-04/V2-05 边界见 [V2-03 验收证据](evidence/v2-03.md)。

## 6. V2-04：Runtime Engine 数据独立

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2R-001 | done | 改造 Execution 创建 | 从 Runtime Bundle 创建 Snapshot | 不读取 Workflow Version/Catalog 控制表 |
| V2R-002 | done | 改造运行授权 | Runtime 授权投影、Policy Epoch、Staleness | 撤权、过期和 Last Known Good 测试 |
| V2R-003 | done | 改造 Resource/Credential Resolver | Runtime Binding + Vault Reference | 无控制库 Secret 回退 |
| V2R-004 | done | 提取可独立启动的 Runtime 后台 Role | command/outbox/recovery/artifact/quota；默认紧凑 Profile 合并运行 | 每个 Role 多副本竞争测试；不要求默认创建独立 Deployment |
| V2R-005 | done | 迁移基础状态机 | Item、Expression、Attempt、Delivery、分支、循环和 Context | M4/Workflow 5.0 基础语义回归 |
| V2R-006 | done | Worker capability 领取边界 | 通用池标签、兼容矩阵、可选拆分 Deployment/外部扩缩容/连接预算 | 不兼容 Worker 不领取任务；默认紧凑 Profile 无强制拆池 |
| V2R-007 | done | Runtime Redis 重建 | Outbox 重投、Pending Recovery、Quota 校准 | 清空 Redis 后执行可收敛 |
| V2R-008 | done | 迁移资源 Runtime | Model、MCP、RAG、Memory、Skill 和 Credential Handle | 精确版本、递归依赖、撤权和无 Control 回退 |
| V2R-009 | done | 迁移 Agent/Sandbox | Agent Loop、Runtime Call、Sandbox Lease 和短凭据 | 预算、取消、恢复和隔离回归 |
| V2R-010 | done | 迁移 Wait/Approval/Resume | Runtime Task、Decision Command、Resume Token | 并发决策唯一终态、响应丢失重试 |
| V2R-011 | done | 迁移 Checkpoint/Fork | Artifact、Reference、部分重跑和副作用确认 | 原执行不变、旧 Bundle 不被 GC |
| V2R-012 | done | 迁移 Evaluation | Evaluation Work Package、Case/Evaluator Execution | Profile/Dataset 固定版本、取消、Runtime 事件；治理投影由 V2-05 完成 |
| V2R-013 | done | 迁移 Composite/Sub-workflow | 依赖闭包、固定子版本和递归保护 | Control 离线运行 Composite |
| V2R-014 | done | 迁移 Debug Work Package | Draft Revision/Overlay/TTL/Debug Queue | Debug 不建立生产 Head、不回读 Control |
| V2R-015 | done | 迁移配额和 Retention | Admission Reservation、Usage Ledger、Hold/GC | 失败恢复零漂移、引用保护 |

### 退出条件

- Coordinator、Runtime Worker、Workflow Worker 和 Sandbox Manager 只连接 Runtime MySQL/Redis/OSS/Vault。
- Runtime 全链路在 Control NetworkPolicy 断开时完成。
- 原功能追踪矩阵中的 Runtime 产品能力逐项通过 V2 等价回归；不得仅以基础 Workflow 成功代替。

完成证据见 [V2-04 验收证据](evidence/v2-04.md) 与 Kubernetes Run `20260815013106`。V2Q、V2S、V2K 及最终切换任务仍按后续阶段独立追踪。

## 7. V2-05：投影、查询和可观测面

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2Q-001 | done | Runtime 当前状态与查询索引事务化 | Invocation/Message/Approval/Evaluation/Execution 当前状态和必要索引 | 状态、索引与 Outbox 同事务；停止所有 Consumer 后 Runtime Query/恢复仍正确 |
| V2Q-002 | done | Runtime→Control Event Pull | Runtime Event/Snapshot Export、Control Cursor/Receipt | Runtime 不调用 Control；Control 不连接 Runtime DB；停机后续拉 |
| V2Q-003 | done | Runtime 列表 Query API | Execution/Invocation 强一致分页、索引和授权 | Control 无 Runtime Summary 表，列表只走 BFF |
| V2Q-004 | done | Runtime Detail BFF | Platform API 调 Runtime Query API | Platform 不直连 Runtime DB |
| V2Q-005 | done | Runtime Trace Relay 与 Consumer | `workflow-runtime --role=trace-relay` + `observability --role=trace-consumer` | Consumer 无 Runtime DB 凭据；并发 Relay 无丢失、CH 故障允许去重 |
| V2Q-006 | done | Observability API | ClickHouse Trace/Cost/Error API | 查询限流、超时、租户隔离 |
| V2Q-007 | done | 治理投影重建 | Approval/Evaluation/Notification、Debug/Retention 结果和必要审计/告警 Snapshot Export + 增量 Cursor | Control 无 Runtime DB 直连完成重建，不复制通用执行列表、成本或 Runtime Status |

### 退出条件

- 列表和实时详情均以 Runtime Query API 为权威，Trace 以 Observability API 为数据源；Control MySQL 不保存 Runtime Summary 列表。
- 完成证据见 [V2-05 Run `20260815112040`](evidence/v2-05.md)，包含 12 个 Kubernetes 场景和 `E2E-V2-009`。
- ClickHouse 故障时 Runtime 完成且控制台明确显示 Trace 延迟。

## 8. V2-06：全服务横向扩展

V2-06A 已完成 `V2S-001～005` 并关闭 `E2E-V2-005/006`；V2-06B 的 `V2S-006` 仍为 `planned`。06A 的功能性 `2→4→2` 不替代容量、公平性和两小时稳定性门禁。

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2S-001 | done（[证据](evidence/v2-06.md)） | 审计全部 Claim/Lease 使用 | Fencing Token、索引、批次和过期接管一致性 | 所有后台任务通过通用协议，无私有扫描漏洞 |
| V2S-002 | done（[证据](evidence/v2-06.md)） | 验证 Trigger/Recovery/Artifact 竞争 | 分片或行 Lease | 多副本不重复外部副作用 |
| V2S-003 | done（[证据](evidence/v2-06.md)） | 验证 Sandbox Reaper | 独占 terminating Claim 和 Provider 对账 | 多 Manager 只终止一次或收敛到一次结果 |
| V2S-004 | done（[证据](evidence/v2-06.md)） | 验证 Control Pull Projector/Trace Relay 竞争 | Cursor/Receipt 与 Outbox Claim | 多副本无热点重复扫描；Runtime 当前状态不依赖 Projector |
| V2S-005 | done（[证据](evidence/v2-06.md)） | PDB/指标契约/优雅终止 | 用户驱动扩缩容、连接预算和 Drain | 紧凑 Profile `2→4→2` 不丢任务/SSE；Agentx 不创建 HPA |
| V2S-006 | planned | 容量与背压 | 租户/Provider/数据库/队列限制 | 过载可解释拒绝，无雪崩 |

### 06A 已通过条件

- 所有常驻 Deployment 以至少两个副本完成故障测试。
- 代码和清单不再依靠单副本保证正确性。
- 七类工作负载完成探针、Drain、PDB 和真实 `2→4→2`；该历史 Run 使用过 HPA，当前部署改为用户或外部平台驱动扩缩容；证据见 [Run `20260816-v206a-final6`](evidence/v2-06.md)。

### 完整阶段退出条件

- `V2S-006` 的容量、背压、公平性和真实两小时稳定性门禁通过。

## 9. V2-07：Kubernetes、运维和安全隔离

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2K-001 | in_progress | 重写 Deployment Profile | V2-07A 已实现外部生产 Profile、七类紧凑应用和独立 Target；真实 E2E 待完成 | Profile Schema、组合渲染和 Role 集合启动测试 |
| V2K-002 | in_progress | 强化 Namespace/NetworkPolicy | V2-07A 完成标准容器与固定 CIDR Egress；强 RuntimeClass 延期 07B | DB/Redis 越界拒绝；允许链路正向测试 |
| V2K-003 | in_progress | 独立 Secret/ServiceAccount | V2-07A 完成工作负载 Secret 和 CA 投影；Role 级拆分与最终供应链延期 07B | Pod 环境和权限扫描 |
| V2K-004 | in_progress | 独立 Migration 流程 | 独立 Expand/Contract Target 已实现；集群竞争待验证 | 并发 Migration 被拒绝 |
| V2K-005 | in_progress | 备份与恢复手册 | RPO/RTO、Manifest 和 Provider Adapter 已冻结；真实恢复待演练 | 恢复演练 |
| V2K-006 | in_progress | Runtime 独立升级 | 独立运维 Action 已实现；持续 Invocation 集群升级待验证 | 持续 Invocation 探针通过 |

### 退出条件

- 可以只安装/升级/查看 Control 或 Runtime Target。
- Runtime 不拥有任何 Control MySQL Secret，也不能访问 Control 数据端口；Control Redis 不存在；只允许冻结的 Internal API 和运行必需的 OSS/Vault/Provider/OpenSandbox 白名单出口。

## 10. V2-08：08A 本地收口与 08B 生产认证

V2-08A 已由 Run `20260818-final4` 完成；V2-08B 的正式容量、生产安全、恢复和最终发布审查保持延期，因此 V2-08 总阶段仍为 `in_progress`。

### 任务

| ID | 状态 | 任务 | 交付物 | 验收 |
|---|---|---|---|---|
| V2C-001 | done | 删除 V1 Schema/API/配置 | 无兼容代码的 V2 工作树；151/151 API 已处置 | 边界、Workspace、OpenAPI 和空库门禁通过 |
| V2C-002 | done | 全新环境初始化 | V2 Bootstrap 和示例 Bundle | Run `20260818-final4` 空环境闭环 |
| V2C-003 | done | 控制面离线故障矩阵 | 停 Web、`platform-control`；静态确认无 Control Redis | 已发布 Runtime 调用继续完成 |
| V2C-004 | done | Runtime 故障矩阵 | Runtime DB/Redis/Worker/OSS/CH/Vault/OpenSandbox | 最终拓扑与专项阶段证据共同证明安全暂停、恢复收敛 |
| V2C-005 | planned（延期到 08B） | 正式多副本容量矩阵 | Gateway/Coordinator/Worker/SSE/Trace | 达到冻结生产基线和两小时门禁 |
| V2C-006 | planned（延期到 08B） | 最终生产文档和发布审查 | 架构、Schema Catalog、Runbook、Evidence | 生产认证矩阵全部 done |
| V2C-007 | done（08A 本地范围） | 产品能力等价回归 | 本地产品矩阵的 V2 全量映射和证据 | 本地功能闭环；生产安全/容量行留在 08B |

### 08A 退出条件

- [V2-08A 证据](evidence/v2-08.md)中的本地功能、故障和性能回归全部通过。
- 不存在 V1 兼容 Flag、双写、旧表、旧 Profile 和跨面数据库访问。
- V2-08B 生产认证完成前，V2-08 和 V2 总阶段保持 `in_progress`。

## 11. 推荐执行顺序

```text
V2-00 契约 / Claim 基础
  ↓
V2-01 分域基础设施 / 最小部署隔离
  ↓
V2-02 Bundle 发布 / 基础垂直切片
  ↓
V2-03 Gateway ─────┐
  ↓                │
V2-04 Runtime Core │
  ↓                │
V2-05 Query/Event ─┘
  ↓
V2-06 Horizontal Scale Verification
  ↓
V2-07 Deployment/Security
  ↓
V2-08 Cutover/Acceptance
```

V2-03 和 V2-04 可以在基础垂直切片通过后并行开发，但必须按产品能力切片逐项合并和删除旧读取，不能等到阶段末一次性关闭全部回归。V2-06 不首次发明 Claim/Lease 正确性，只负责全服务审计、竞争强化和规模化验证。
