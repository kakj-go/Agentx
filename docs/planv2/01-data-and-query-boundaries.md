# V2 数据、中间件与查询边界

## 1. 数据所有权规则

1. 一份事实只有一个权威写入方。
2. Runtime MySQL 中的 Control 数据都是发布投影或授权投影，不是 Control 配置权威。
3. Control MySQL 只保存审批、评测、通知、Debug/Retention 操作结果、审计和告警等治理结果投影；不复制通用 Execution/Invocation 列表、成本或 Runtime Status Read Model。
4. 跨面 ID 是逻辑关联，不建立跨库外键。
5. 浏览器不能负责拼接关键一致性结果；聚合由对应 Query API 完成。

以上查询边界已在 V2-05 实现并通过停止 Consumer、强一致分页、Projection Snapshot 重建、ClickHouse 停机和 Credential/NetworkPolicy 正负向验证；证据见 [V2-05 Run `20260815112040`](evidence/v2-05.md)。

## 2. Control MySQL

Control MySQL 保存“设计和治理事实”：

| 领域 | V2 权威对象 |
|---|---|
| IAM | Tenant、User、Department、Role、Permission、Data Scope |
| Workflow 设计 | Workflow、Draft、Draft Revision、Editor Document、Debug Overlay |
| Catalog | Node Definition、Node Manifest Version、UI Schema |
| 资源 | Credential Metadata、Model、MCP、Skill、RAG、Memory、Sandbox Profile |
| 授权治理 | Workflow Service Identity、Resource Grant、Grant Request、Review |
| 发布 | Workflow Version、Control Deployment、Runtime Bundle、Publish Attempt |
| 应用配置 | Application Metadata、环境、发布策略、控制面的 Key 管理记录 |
| 测试治理 | Dataset、Evaluation Profile、Evaluation Definition |
| 控制审计 | Audit、Notification Preference、Control Outbox/Inbox |
| 治理投影 | Notification、Approval/Evaluation、Debug/Retention 操作结果、必要审计/告警和关联索引；不保存通用 Runtime Execution/Invocation Summary、Cost 或 Runtime Status |

V2 应从空库建立 Control Migration，不保留当前全量 Migration 链。旧表只在删除清单中记录，不编写数据搬迁程序。

## 3. Runtime MySQL

Runtime MySQL 保存“生产服务和执行事实”：

| 领域 | V2 权威对象 |
|---|---|
| 发布投影 | Runtime Bundle、Deployment Head、Runtime Application Route |
| 入口安全 | API Key Hash、Webhook Secret Reference、Runtime JWT Policy、Revocation Version |
| Trigger | Webhook、Schedule、Poll、Lifecycle Binding 和扫描租约 |
| 会话 | Session、Session Context、Message、Message Part |
| 调用 | Invocation、Invocation Event、Idempotency Record、Runtime Command |
| 执行 | Execution、Execution Snapshot、Node Execution、Attempt、Edge Delivery、Lineage |
| 可靠性 | Execution Outbox、Worker Lease、Wait、Resume Token、Checkpoint、Fork |
| 运行授权 | Service Identity Projection、Resource Grant Projection、Resource State、Policy Epoch |
| 资源运行 | 精确 Resource Binding、Secret Reference、Runtime Call、Credential Handle |
| 配额 | Quota Policy Projection、Reservation、Usage Ledger |
| 运行协作与查询 | Approval Runtime、Evaluation Case Runtime、必要的查询索引；与对应领域状态同事务写入 |
| 可观测投递 | Execution Event、Trace Delivery Outbox、Runtime-to-Control Outbox |
| 运维 | Worker Capability、Service Heartbeat、Maintenance Lease、Runtime Inbox |

Runtime 表不得引用 Control 表。Bundle 激活时必须把执行所需的数据完整物化到 Runtime 数据模型。

## 3.1 跨面业务权威

跨面业务不能用“两边各存一份状态”含糊处理：

| 业务 | Control 权威 | Runtime 权威 | 协作方式 |
|---|---|---|---|
| Application | 元数据、发布策略、Key 管理记录 | Route、Head、启停状态、Key Hash、调用状态 | Publish/Admission Command |
| Approval | 当前 IAM、候选资格规则、操作审计、控制台投影 | Approval Task 生命周期、Decision 接受结果、Resume | Control 校验操作者后发送带 Task Version 的 Decision Command；Runtime CAS 决定唯一终态并回传结果 |
| Evaluation | Dataset/Profile/Definition、Run Intent、报告投影 | Case、Evaluator Execution、实际输出和运行终态 | 不可变 Evaluation Work Package + Runtime Event |
| Notification | 用户偏好、站内通知和已读状态 | 只产生需要通知的 Runtime Event | Control Inbox 同事务创建通知，Runtime 不维护第二份已读状态 |
| Audit | Control 配置和人工操作审计 | Runtime Admission、执行恢复和系统决策审计 | 分域保存；通过 Correlation ID 联查，不互相覆盖 |
| Debug/Fork | Draft/Overlay、发起权限和操作记录 | Work Package、Execution、Checkpoint/Fork 结果 | 受控 Internal API + Inbox/Outbox |

Approval Action 在 Control 创建时只能标记为 `submitted`，Runtime 接受对应 Task Version 后才形成最终 `accepted/rejected/conflicted` 结果。HTTP 响应丢失时使用同一 Idempotency Key 查询结果，禁止 Control 先把 Runtime Task 推定为完成。

## 3.2 当前表逐表处置目录

V2-00 以 [历史 V1 MySQL Schema Catalog](../reference/mysql-schema-catalog.md) 为输入生成并冻结 [逐表处置目录](contracts/table-disposition.json)。该历史目录中的每张表必须恰好出现一次，并包含：

```text
current_table
decision = control | runtime | split | delete
v2_table_or_replacement
authoritative_writer
allowed_readers
cross_plane_event
retention_and_delete_rule
owning_task
```

没有逐表结论的 Schema 不得进入 V2-01 Migration。静态 SQL 门禁使用这份机器可读目录生成允许列表，不能仅依赖表名前缀猜测归属。

## 4. Redis 归属

Control 不部署 Redis：

- Control MySQL 保存 Outbox、Inbox、发布任务、投影 Cursor、幂等记录和 Claim/Lease。
- 控制配置查询可以使用带 TTL/Version 的有界进程内缓存；Pod 重启后直接回查 Control MySQL。
- Ingress/API 负责普通限流，MySQL 只承担低频且需要全局一致的安全额度。
- Control 服务不得连接 Runtime Redis。

Control 原 Redis 用途按语义替换，不做 Key 到表的机械迁移：

| 用途 | V2 实现 | 约束 |
|---|---|---|
| 发布任务、Outbox/Inbox | Control MySQL 状态表 + `SKIP LOCKED`/Lease | 有索引、有批次、有重试上限，不能忙轮询 |
| Idempotency、一次性 Token、登录/Key 安全状态 | Control MySQL 唯一键、状态和过期时间 | 事务提交决定结果；定期分批清理过期行 |
| Refresh Session/撤销记录（若采用服务端会话） | Control MySQL Hash/Version/Expiry | 不保存明文 Token；查询必须命中覆盖索引 |
| 分布式锁/Leader | Control MySQL Lease + Fencing Token | 只用于低频协调，不持锁执行长事务或外部调用 |
| 配置与权限读取缓存 | 每 Pod 有界 TTL/Version Cache | 非权威；失效或重启后回查 Control MySQL |
| 普通 HTTP 限流/突发保护 | Ingress + API 本地 Token Bucket | 不逐请求写 MySQL；允许副本间近似误差 |
| 登录失败、Bootstrap、Key 创建等全局安全额度 | Control MySQL 条件更新 | 仅低频路径；压测超过阈值时先调整安全策略和索引 |
| 缓存失效通知/Pub/Sub | 短 TTL 或版本检查 | 首期不为 Control 引入高频 Pub/Sub；不能连接 Runtime Redis 代替 |

如果某项需求必须具备每请求全局精确计数、亚毫秒共享缓存或高频 Pub/Sub，它不属于“MySQL 直接替代”的适用范围；V2 首期应调整需求或调用路径，而不是把压力转移到 Control MySQL。

### 4.1 Runtime Redis

允许用途：

- capability 分片的 Node Task Stream。
- Runtime Event/SSE 唤醒。
- Trace Stream。
- 短期配额 Admission Counter。
- 非权威缓存。

所有 Node Task 都必须先有 Runtime MySQL Outbox；Worker 消费后必须回查 Runtime MySQL 并取得 Attempt Lease。

## 5. OSS/S3 归属

| 存储域 | 内容 | 主要写入者 | 主要读取者 |
|---|---|---|---|
| Control | Skill Workspace、Package、Dataset Source、导入导出、控制附件 | Platform API、Control Worker | Platform API |
| Runtime | Binary Item、Checkpoint、Node Output、Session File、Runtime Response | Gateway、Coordinator、Worker | Runtime Query、Worker |
| Observability | 可选 Trace 大载荷和归档 | Trace Writer | Observability API |

对象元数据分别保存在所属 MySQL。一个面不得直接使用另一个面的数据库元数据解析对象。

## 6. ClickHouse 归属

ClickHouse 保存：

- Workflow/Node Trace Event。
- Agent Iteration。
- Model Token、Cost 和 Provider Timing。
- MCP/RAG/Memory/Sandbox 调用分析。
- 错误、耗时、容量和租户聚合。

使用稳定的 `event_id` 和 `execution_id` 去重/关联。ClickHouse 不保存唯一 Session、Invocation 或 Execution 状态。

## 7. 查询路由

### 7.1 控制配置查询

```text
Browser → Platform API → Control MySQL / Control OSS
```

用于 Workflow、Draft、Catalog、资源、Grant、Version、发布历史和评测配置。

### 7.2 生产运行查询

```text
Client → Runtime Gateway/Query API → Runtime MySQL / Runtime OSS
```

用于 Invocation、Session、Message、Execution、Attempt、Wait、Checkpoint、最终输出和 SSE。不得经过 Platform API。

### 7.3 控制台运行列表

```text
Browser → Platform API/BFF → Runtime Query API → Runtime MySQL
```

用于 Execution/Invocation 列表、当前状态、最近运行和轻量运行摘要。首期与详情共享 Runtime Query API，避免为了拆库立即复制整套运行数据。成功率、成本和长时间范围聚合优先由 Observability API 提供。

V2 不把列表复制到 Control MySQL。若未来 Runtime Query 成为瓶颈，优先增加只读副本、覆盖索引、缓存或专用 Runtime Query Store；任何新增跨面 Read Model 都必须作为新的 ADR 单独评审，不在 V2 中预留双路径。

这里所说的 `Control Runtime Read Model`，是把 Runtime 中的 Execution/Invocation 摘要通过事件或快照复制到 Control MySQL，形成只读查询副本。它能让控制台在 Runtime Query 不可用时浏览旧摘要，也能降低 Runtime MySQL 的列表查询压力；但会同时引入投影延迟、Cursor/Receipt、重建流程、保留策略、字段演进和“列表与详情暂时不一致”的产品语义。

V2 首期不实现这套通用 Read Model：运行列表和详情都以 Runtime Query API 为唯一查询路径，避免同一页面存在两份来源。Control MySQL 只保留 Approval、Evaluation、Notification、Debug/Retention 操作结果和必要审计/告警等需要参与控制面治理流程的投影，这些投影不能替代 Runtime Execution 当前状态，也不能形成 Cost 或 Runtime Status 副本。

Runtime 内部也不采用“先写 Event、再由异步 Projector 生成当前状态”的路径。Invocation、Session Message、Approval Task、Evaluation Case 和 Execution 查询所需的当前字段，在对应领域事务内同步写入；同事务 Outbox 仅负责把已提交事实导出到 Control 或 Observability。确需异步生成的只能是可丢弃并可重建的分析数据，且不能影响 Runtime Query 正确性、状态推进或恢复。

### 7.4 控制台实时详情

```text
Browser → Platform API/BFF → Runtime Query API → Runtime MySQL
```

Platform API 只做代理鉴权和响应组合，不直连 Runtime MySQL。也可以由浏览器取得短期 Runtime Query Token 后直接访问 Runtime API；V2 首期优先采用 BFF，减少浏览器 Token 域复杂度。

BFF 调用携带短期 Delegation Token，限定 Tenant、Subject、操作和允许访问的 Execution/Session/Application 范围。Runtime Query API 必须再次校验 Scope 并写查询审计，不能仅凭 Platform API 的网络来源放行。

### 7.5 Trace 查询

```text
Browser → Observability API → ClickHouse
```

若页面需要同时显示终态和 Trace：

1. Runtime Query API 返回权威 Execution 状态。
2. Observability API 返回 Trace。
3. Trace 暂不可用时仍显示 Execution 状态并明确标记同步延迟。

## 8. 关键查询映射

| 查询 | 服务 | 数据源 | 一致性 |
|---|---|---|---|
| Workflow Draft | Platform API | Control MySQL | 强一致 |
| 当前生产 Deployment | Runtime Gateway | Runtime MySQL Head | 强一致 |
| API Key 是否有效 | Runtime Gateway | Runtime MySQL/有版本缓存 | 强一致或单调缓存 |
| 新 Execution 授权 | Coordinator | Runtime 授权投影 | 强一致 |
| Attempt 重试授权 | Worker/Coordinator | Runtime 授权投影 | 强一致 |
| Invocation 终态 | Runtime Gateway | Runtime MySQL | 强一致 |
| 控制台运行列表 | Platform API/BFF → Runtime Query API | Runtime MySQL | 强一致分页快照 |
| Node Output | Runtime Query API | Runtime MySQL + Runtime OSS | 强一致引用 |
| Trace | Observability API | ClickHouse | 最终一致 |
| 配额 Admission | Coordinator/Worker | Runtime Redis + Runtime MySQL校准 | 快速判定，MySQL权威 |

## 9. 跨面事件

### 9.1 Control→Runtime

| 事件/命令 | 作用 |
|---|---|
| `bundle.prepared` | 安装不可变 Bundle |
| `deployment.activated` | 原子切换生产 Head |
| `deployment.rolled_back` | 切换到旧 Bundle |
| `application.disabled` | 拒绝新 Invocation |
| `application.enabled` | 在现有单调 Admission Epoch 上恢复接受新 Invocation |
| `api_key.upserted/rotated` | 安装新 Key Hash/版本并按轮换策略保留或废止旧版本 |
| `api_key.revoked` | 更新 Runtime Key 状态 |
| `resource.grant_updated` | 安装新的 Grant/Resource State 和 Policy Epoch |
| `resource.revoked` | 更新运行授权/资源状态 |
| `quota_policy.updated` | 更新 Runtime 配额投影 |
| `tenant.runtime_disabled` | 拒绝租户的新 Runtime 请求 |
| `tenant.runtime_enabled` | 使用更高 Tenant Admission Epoch 恢复运行 |
| `approval.decision_submitted` | 对 Runtime Approval Task 做带版本的唯一终态决策 |
| `evaluation.run_requested/cancelled` | 创建或取消不可变 Evaluation Work Package |
| `debug.run_requested/cancelled` | 创建或取消带 TTL 的 Debug Work Package |
| `session.bundle_upgrade_requested` | 显式升级固定版本 Session 使用的 Bundle |
| `retention.hold/release` | 对指定 Runtime Aggregate 建立或释放保留锁 |

### 9.2 Runtime→Control（Control Pull）

| 事件 | Control 投影 |
|---|---|
| `invocation.accepted/completed/failed` | 控制面通知、治理审计或关联索引；不建立通用 Invocation 列表 |
| `execution.started/terminal` | 治理审计、告警或关联索引；不建立通用 Execution 列表 |
| `approval.requested/resolved` | 控制台待办与历史 |
| `evaluation.case.terminal` | Evaluation 报告摘要 |
| `runtime.health.changed` | 必要告警/通知；Runtime Status 页面仍走 Runtime Query/Observability API |
| `approval.decision_applied/conflicted` | 人工操作最终结果和审计投影 |
| `evaluation.run.progress/terminal` | Evaluation Run/Report 投影 |
| `debug.run.terminal` | Studio Debug 结果索引 |
| `retention.completed/failed` | Runtime 清理结果 |

Runtime 不主动调用 Control。上述事件先以单调 Runtime Cursor 写入 Runtime Integration Event Log；`platform-control --role=projector` 使用受限 Event Export API 分页或 Long Poll 拉取，Receipt、业务投影和新的 Control Cursor 在同一 Control MySQL 事务提交。控制面停机不会让 Runtime 产生请求重试风暴，也不需要 Runtime 持有 Control 身份。Integration Event Log 不是领域事实日志，不能重建 Runtime 当前状态。

所有事件统一 Envelope：

```text
event_id
schema_version
source_plane
tenant_id
aggregate_type
aggregate_id
object_version
occurred_at
payload
content_hash
correlation_id
causation_id（可选）
idempotency_key（命令必填）
```

跨面 Command/Event 默认携带自包含的目标状态、终态结果或不可变对象引用，不发送必须依赖完整历史才能解释的增量 Patch。`object_version` 用于拒绝旧状态覆盖新状态；Runtime Event Export 另有全局单调 Cursor 用于分页和断点续拉。该协议是状态同步与可靠集成，不是 Event Sourcing Log。

### 9.3 顺序、重复和冲突规则

- Inbox 以 `source_plane + event_id` 唯一，Receipt 与业务应用在同一数据库事务提交。
- Control→Runtime Command 使用单调 `object_version`、业务 Epoch 或 CAS Head Version；小于等于已应用值的命令返回既有结果，不能覆盖新状态。命令携带自包含目标状态时允许版本跨越，不要求逐个补齐中间版本。
- Runtime→Control Export 使用全局 Cursor 拉取、自包含事件和对象 Version Upsert。Control 不以事件历史重建 Runtime 当前状态；旧版本事件可幂等忽略，Cursor 超出保留窗口时只对治理投影执行 Snapshot Export。
- 只有契约明确声明为有序增量 Patch 且 ADR 证明全量状态不可行时，才允许要求连续 Sequence 和 Gap Repair；V2 首期不包含这类通用协议。
- 相同 `event_id` 携带不同 `content_hash` 属于安全错误，进入 Dead Letter/人工处置，禁止覆盖原 Receipt。
- Admission Command 使用独立单调 Epoch。Bundle Rollback、事件重放和迟到消息都不能让 API Key、Grant、Tenant Status 或 Quota Epoch 倒退。
- Control→Runtime Command Outbox 只有在 Runtime 返回“已在同一事务应用”或可验证的既有 Receipt 后才能标记完成；HTTP 超时视为结果未知，必须以相同 Idempotency Key 重试。
- Runtime→Control Event 使用拉取 Cursor：Control 只有在 Receipt 与投影事务提交后才推进自己的 Cursor；重复拉取必须命中 Receipt，不能重复业务事实。

### 9.4 治理投影重建与保留

- Approval、Evaluation、Notification、Debug/Retention 操作结果和必要审计/告警等 Control 治理投影可从 Runtime Integration Event Log 的保留窗口内增量重放。
- 全量损坏时，Control 通过受限 Snapshot Export API 按 Tenant 和 Cursor 获取对应治理对象基线，再继续消费增量事件；禁止临时开放 Runtime MySQL 直连。
- Runtime Outbox/Inbox 的最短保留期、最大积压容量和 Snapshot 周期在 V2-00 容量决策中冻结，并且必须覆盖已声明的最大控制面离线时间。
- 治理投影重建期间对应页面返回 `projection_status=rebuilding`、`projected_at` 和 Cursor，不得把缺失记录显示为真实不存在。Execution/Invocation 列表仍直接走 Runtime Query BFF，不依赖治理投影。

## 10. OSS 制品发布边界

Control OSS 保存编辑源文件，Runtime OSS 保存已发布、内容寻址的运行制品。发布流程必须：

1. Builder 解析依赖闭包并生成 Object Manifest。
2. 以 Runtime IAM 将对象复制/上传到 Runtime Domain，逐项校验 Hash 和 Size。
3. Runtime Prepare 验证所有必需对象可读后才保存 `prepared` Bundle。
4. Activate 只切换数据库 Head，不执行大对象复制。
5. Object Key 不包含 Secret；相同 Tenant 和 Hash 可以安全复用，跨 Tenant 默认不复用授权。

Runtime Artifact、Checkpoint、Node Output 和 Session File 继续由 Runtime 产生。Control 或 Observability 下载时必须通过 Runtime Query/短期签名 URL 授权，不能直接共享 Runtime OSS 长期 Credential。

## 11. Retention、删除和跨库引用保护

- Control 删除或禁用 Application、Resource、Tenant 时先写命令和 Tombstone；Runtime 应用禁用/撤权后回传 Receipt。历史 Execution 按 Runtime Retention 保留。
- 没有跨库外键时，删除保护由 Control Reference Index、Runtime Reference API 和最终事务复检共同完成；查询超时默认 Fail Closed。
- Bundle、Runtime Object、Checkpoint Source、固定版本 Session 和等待中的 Execution 使用显式 Reference/Retention Hold，GC 只删除 `garbage_collectable` 对象。
- Runtime Execution、Message、Artifact、Audit、Outbox/Inbox 和 Trace 分别定义最短/最长保留期；Outbox/Inbox 的保留期不能短于跨面离线恢复目标。
- 删除是幂等后台任务。对象存储删除失败不回滚已提交的业务 Tombstone，由可认领任务重试并保留最终残留报告。

## 12. 缓存规则

- Deployment Head、API Key 和授权缓存必须带 Runtime MySQL 中的版本或 Epoch。
- 禁止没有 TTL/Version 的永久进程内缓存。
- 缓存失效时回查本地所属数据库，不回查另一个面。
- Runtime MySQL 不可达时，Gateway 不允许仅凭陈旧缓存接受需要持久化的新 Invocation。
