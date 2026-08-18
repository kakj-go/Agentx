# Agentx V2 控制面与执行面分离重构总计划

## 1. 目标

V2 已在 08A 将原“服务进程分离、共享 MySQL”的架构重构为相互独立的控制面、执行面和可观测面；以下生产级条件仍需由 08B 最终认证：

1. Web Console、Platform Control 和 Control MySQL 全部不可用或正在升级时，执行面仍能使用最后一次成功激活的发布制品接收生产请求并完成 Workflow。
2. 生产请求链路不访问 Control MySQL 或 Platform Control。
3. 控制面与执行面拥有独立 MySQL、Migration、凭据、入口、扩缩容和故障边界；V2 不部署 Control Redis，Runtime Redis 是执行面独占依赖。
4. 除中间件、Migration Job 和采用 Leader Lease 的全局维护任务外，所有常驻应用服务都能安全部署多个副本。
5. Redis、ClickHouse 和控制面投影都不是 Execution 权威状态；Runtime MySQL 始终能够解释一次执行的当前状态和下一步。
6. V2 重构不减少现有产品能力；架构隔离矩阵和产品能力等价矩阵必须同时完成。

本计划以当前工作树为分析基线，但不要求兼容当前开发数据、数据库 Schema、内部代码 API、HTTP API、gRPC、Runtime Event 或部署 Profile。实现时允许删除旧表、旧 Migration、旧 DTO、旧查询和过渡 Adapter，直接建立 V2 空库。

“不兼容”只针对数据和技术契约，不代表默认删除已有产品能力。除本计划明确排除的内容外，Workflow 4.0、Studio、Application、Session、Evaluation、Approval、Checkpoint/Fork、Agent、资源 Runtime、Trace 和 OpenSandbox 等现有用户能力必须在 V2 主链上重新闭环；允许以新的 API、表和内部实现完成。

## 2. 固定基础设施拓扑

| 中间件 | 数量 | 归属 | 说明 |
|---|---:|---|---|
| MySQL | 2 | Control、Runtime 各一套 | 必须是两个独立实例或集群；同实例双 Schema 不满足故障隔离 |
| Redis | 1 | Runtime 独占 | Control 不部署 Redis，也不得连接 Runtime Redis |
| OSS/S3 | 1 | 三个面共享物理服务 | 使用独立 Bucket/Prefix、账号和 IAM Policy 隔离 Control、Runtime、Observability |
| ClickHouse | 1 | 可观测面 | 只保存 Trace 与分析数据；不可用不得改变 Execution 结果 |

Vault/Secret Provider、OpenSandbox、模型、MCP、LightRAG 和 Mem0 是外部执行依赖，不计入上述四类权威中间件数量。

## 3. 目标部署单元

### 3.1 控制面

- `web-console`：静态控制台和 Studio。
- `platform-control`：同一构建产物以 `api` Role 承载控制配置、IAM、草稿、资源、版本、发布操作和查询，以 `publisher`、`projector`、`retention` Role 承载后台任务；只通过 Runtime Internal API 交互，不连接 Runtime MySQL。
- `control-migrate`：Control MySQL 一次性 Migration Job。

### 3.2 执行面

- `runtime-gateway`：Application API、API Key、Webhook、Session、Invocation、SSE、Cancel 和 Wait Resume。
- `workflow-runtime`：同一构建产物以 `coordinator` Role 承载 Execution 创建、状态机推进和节点结果提交，以 `trigger`、`command`、`outbox`、`recovery`、`artifact`、`quota`、`trace-relay` Role 承载后台任务。Runtime 当前状态和必要查询表在领域事务内同步写入，不设置异步 Runtime Projector。
- `workflow-worker`：按 capability 消费节点任务和执行节点。
- `sandbox-manager`：OpenSandbox 生命周期和短期凭据注入。
- `runtime-migrate`：Runtime MySQL 一次性 Migration Job。

### 3.3 可观测面

- `observability`：同一构建产物以 `trace-consumer` Role 消费 Runtime Redis Trace Stream并写 ClickHouse，以 `query` Role 提供 Trace、成本、错误和聚合查询；不持有 Runtime MySQL 凭据。
- `clickhouse-migrate`：ClickHouse 一次性 Migration Job。

V2 首期应用构建产物预算固定为 7 类：`web-console`、`platform-control`、`runtime-gateway`、`workflow-runtime`、`workflow-worker`、可选 `sandbox-manager` 和 `observability`。各 Role 可以为了副本数、资源或权限使用不同 Deployment，但共享对应二进制、版本和基础模块，不为每个 Role 建立微服务。超过该预算必须新增 ADR，证明独立安全边界或负载隔离收益。

首期默认使用紧凑 Profile：每类应用构建产物只创建一个常驻 Deployment，`platform-control`、`workflow-runtime` 和 `observability` 在同一 Deployment 内启动各自 Role 集合，`workflow-worker` 先使用通用 capability 池。只有权限隔离、资源类型或实测扩缩容指标证明有收益时，才把某个 Role/Capability 拆为单独 Deployment；拆分不新增镜像、业务 API、数据库所有权或发布版本。Migration Job 不计入常驻服务数量。

## 4. 文档目录

| 文档 | 内容 |
|---|---|
| [00-target-architecture.md](00-target-architecture.md) | 架构决策、故障模型、发布制品和最终调用链 |
| [01-data-and-query-boundaries.md](01-data-and-query-boundaries.md) | 表和中间件归属、跨面同步、查询路径与一致性 |
| [02-services-and-scalability.md](02-services-and-scalability.md) | 服务职责、横向扩展协议、后台任务和容量边界 |
| [03-refactor-phases.md](03-refactor-phases.md) | 分阶段任务、依赖、删除项、交付物和退出条件 |
| [04-e2e-acceptance.md](04-e2e-acceptance.md) | 临时 Kubernetes E2E、故障注入、容量和发布门禁 |
| [99-traceability.md](99-traceability.md) | V2 能力到任务、服务、存储和测试的追踪矩阵 |
| [tasks/README.md](tasks/README.md) | V2-00～V2-08 可领取的分步实施任务、依赖、交付物和阶段门禁 |
| [contracts/table-disposition.json](contracts/table-disposition.json) | 当前 133 张 MySQL 表的机器可读唯一处置目录 |
| [contracts/service-runtime-contracts.md](contracts/service-runtime-contracts.md) | 7 类构建产物、Role、端口、Secret、后台循环、容量与恢复契约 |
| [contracts/version-compatibility.md](contracts/version-compatibility.md) | Bundle/IR/Event/Internal API/Worker 版本窗口和 Expand/Contract 顺序 |
| [contracts/backup-provider-adapter.md](contracts/backup-provider-adapter.md) | V2-07A 外部托管备份、恢复、Redis 重建和证据 Receipt 契约 |
| [contracts/v2-07-scenario-adapter.md](contracts/v2-07-scenario-adapter.md) | V2-07A 真实业务连续性、网络、密钥轮换和恢复对账 Scenario Adapter 契约 |
| [contracts/v1-deletion-ledger.md](contracts/v1-deletion-ledger.md) | V1 API/Profile/Adapter/Projector/共享依赖删除台账 |
| [contracts/claim-lease-guide.md](contracts/claim-lease-guide.md) | MySQL Claim/Lease/Fencing 公共协议 |
| [contracts/boundary-policy.json](contracts/boundary-policy.json) | Cargo、SQL、Env、Secret、NetworkPolicy 冻结例外基线 |
| [evidence/egress-gateway.md](evidence/egress-gateway.md) | V2alpha3 受控公网出口、真实模型、密钥轮换、30分钟稳定性与当前 CNI 限制证据 |

## 5. 实施原则

1. **发布时物化，执行时本地读取**：控制面在发布时生成不可变 Execution Spec Bundle 和 Runtime OSS 制品闭包；执行面不在 Invocation 到达后拼装控制面数据。
2. **数据所有权唯一**：每张权威表只属于 Control MySQL 或 Runtime MySQL；禁止跨库 SQL JOIN、外键和分布式事务。
3. **异步跨面**：Control→Runtime Command 使用 Outbox/Internal API/Inbox；Runtime→Control Event 由 Control 按 Cursor 从 Runtime Event Export API 拉取并幂等投影。
4. **执行面自包含**：Application Route、API Key Hash、Trigger、Session、Invocation、运行授权、配额和 Execution 状态全部在 Runtime MySQL。
5. **查询分级**：控制配置查 Control API；运行列表和实时详情经 BFF 查 Runtime Query API；Trace 和长时间聚合查 Observability API。V2 不建设 Control Runtime Read Model。
6. **无本地权威状态**：常驻应用进程可以随时重启或替换，正确性来自数据库状态、租约、幂等键和 Consumer Group。
7. **默认多副本安全**：不能依靠 `replicas: 1` 保证正确性。单实例任务必须改为行级 Claim、分区消费或 Leader Lease。
8. **直接清理旧架构**：不保留旧共享 MySQL 路径、旧 Runtime Projector、旧 Web 反向代理生产入口或旧 Profile 双模开关。
9. **执行制品与动态安全状态分离**：Bundle 回滚只能切换执行 Spec，不能恢复旧 API Key、Grant、Tenant Status、Policy Epoch 或 Quota。
10. **受控跨面通信**：数据库和 Redis 完全隔离；Publish、Event Export Pull、Runtime Query 和 Work Package 只通过独立 Internal API、工作负载身份和 NetworkPolicy 白名单通信。跨面调用统一由 Control 发起，Runtime 不需要 Control DNS、身份或网络出口。
11. **逐能力垂直迁移**：不做生产双写，但每个阶段保持编译、Migration 和测试可运行；替代链路完成后再删除对应旧路径。
12. **不引入消息平台和事件溯源**：V2 不使用 Kafka/Pulsar/NATS，不采用 Event Sourcing；MySQL 领域表/状态机表保存当前权威状态，Outbox/Event 只负责可靠集成、审计和治理投影。
13. **当前状态不异步自投影**：Runtime 的 Invocation、Message、Approval、Evaluation 和 Execution 当前状态随领域事务同步提交；不等待消费自身 Event 后才可查询或继续执行。

## 6. 阶段状态

状态只允许 `planned`、`in_progress`、`blocked` 和 `done`。V2-00～V2-05 已完成代码、契约、全量静态门禁与可复现 Kubernetes E2E；V2-06A 已完成多副本正确性，V2-08A 已完成本地功能闭环和 V1 物理删除。V2-06B 容量/两小时稳定性、V2-07B 生产安全与恢复认证以及 V2-08B 最终生产发布审查尚未执行，因此 V2 总计划仍为 `in_progress`。

| 阶段 | 范围 | 状态 | 核心退出条件 |
|---|---|---|---|
| V2-00 | 契约、逐表归属、Claim 和信任边界 | done（[证据](evidence/v2-00.md)） | Spec/Admission、表归属、跨面 API、版本窗口和产品矩阵冻结 |
| V2-01 | 分域基础设施、空库 Schema 和最小部署隔离 | done（[证据](evidence/v2-01.md)） | 2 MySQL、1 个独占 Runtime Redis、OSS/CH、账号、Migration、NetworkPolicy 可独立验证 |
| V2-02 | Runtime Bundle 和基础垂直切片 | done（[证据](evidence/v2-02.md)，Run `20260813v202p`） | 发布、激活、Gateway→Execution→Query、控制面离线、强退接管和幂等重放闭环 |
| V2-03 | Runtime Gateway 与生产业务状态 | done（[证据](evidence/v2-03.md)，Run `20260813152731`） | `/gateway/v1`、Session/Message/Invocation、Trigger、SSE 和生产入口只访问 Runtime 依赖 |
| V2-04 | Runtime Engine 数据独立 | done（[证据](evidence/v2-04.md)，Run `20260815013106`） | 完整 Runtime Engine 在 Control 离线时运行；真实依赖、故障恢复、引用/配额/保留均收敛 |
| V2-05 | 投影、查询与可观测面 | done（[证据](evidence/v2-05.md)，Run `20260815112040`） | Runtime 权威列表/详情、Control 治理 Generation、Trace/聚合和故障降级边界稳定 |
| V2-06 | 全服务横向扩展验证 | in_progress（[06A 证据](evidence/v2-06.md)，`V2S-006` planned） | 06A 已通过 Claim/多副本/Drain/PDB 和历史 `2→4→2`；当前由用户平台扩缩容，Agentx 只暴露 metrics；06B 仍需容量、公平性、背压和两小时稳定性 |
| V2-07 | Kubernetes、运维和安全隔离 | in_progress（[07A 证据](evidence/v2-07.md)） | 07A 代码与静态门禁已通过，外部 TLS/恢复 E2E 与 07B 强隔离、Role 级权限和供应链门禁仍待完成 |
| V2-08 | 08A 本地功能收口 / 08B 生产认证 | in_progress（[08A 证据](evidence/v2-08.md)，Run `20260818-final4`） | 08A 已补齐 151/151 公共 API、物理删除 V1 并通过本地完整闭环；08B 关闭生产容量、安全、恢复和最终发布认证 |

## 7. 全局完成定义

V2 只有同时满足以下条件才能完成：

- Runtime Pod 没有 Control MySQL 凭据，NetworkPolicy 也禁止访问对应数据端口；Control Pod 不持有 Runtime Redis 凭据。
- 生产域名不经过 Web Console 或 Platform API。
- 已发布 Application 在控制面全部缩容为零、Control MySQL 停机期间仍能连续执行。
- Runtime MySQL 不可用时请求明确失败或安全暂停，不产生伪成功、孤立 Invocation 或重复副作用。
- 所有常驻服务至少以两个副本通过重复投递、Pod 强退、Lease 过期和滚动发布测试。
- Worker 可以按 capability 独立扩容，旧/不兼容 Worker 不领取未知任务。
- Runtime Redis 丢失后能从 Runtime MySQL Outbox 和状态重建；ClickHouse 故障不影响执行。
- Control/Runtime/ClickHouse Migration 互相独立，且每个目标同一时间只运行一个 Job。
- 完整 E2E 使用临时 Namespace；测试结束删除临时 Namespace并恢复被缩容的开发服务。
- 旧共享数据库访问、兼容层、废弃表、废弃 API 和废弃部署清单全部删除。
- Bundle 回滚不会恢复任何已经撤销/禁用的 Admission State，运行制品依赖闭包在 Control OSS 断开时仍完整可用。
- Studio Debug、Evaluation、Approval、Checkpoint/Fork、Composite、Agent 和资源 Runtime 等原有能力均通过 V2 产品能力等价回归。
- 跨面只开放冻结的 Internal API；Runtime 不能访问 Control MySQL，也不主动调用 Control 服务；Control Redis 根本不存在，Control 也不能连接 Runtime Redis；可观测面不持有 Runtime MySQL Credential。
- 容量、恢复时间和积压阈值在测试前冻结并全部通过，不能只以“已记录结果”代替门禁。

## 8. 与现有计划的关系

- `docs/plan/` 保存当前产品能力的实施历史和已完成验收证据，是 V2 产品等价回归基线。
- `docs/planv2/` 是下一阶段控制面/执行面分离重构的唯一执行计划和状态来源。
- V2 实施不回写旧阶段状态；每项原有能力必须在 `planv2/99-traceability.md` 重新取得 V2 证据后才能标记 `done`。
- `docs/13-architecture-service-data-map.md` 已在 V2-08A 按当前 V2 代码、清单和分域 Schema 重写；生产认证未完成项仍以本计划的 08B 状态为准。
