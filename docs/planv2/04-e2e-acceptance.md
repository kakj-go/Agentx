# V2 Kubernetes E2E 与验收标准

## 1. 测试环境

每次完整 V2 验收创建唯一临时环境：

```text
agentx-v2-e2e-control-<run-id>
agentx-v2-e2e-runtime-<run-id>
agentx-v2-e2e-observability-<run-id>
agentx-v2-e2e-deps-<run-id>（需要时）
```

资源不足时允许先缩容 `agentx` Namespace，但测试脚本必须：

1. 记录原副本数。
2. 只操作明确列出的 Deployment。
3. 无论成功失败都恢复副本数。
4. 测试结束删除本次临时 Namespace，不删除共享外部依赖。

业务对象必须通过 UI、公开 API 或正式 Internal Publish API 创建；SQL 只允许环境准备、故障注入后的权威断言和清理。

## 2. 核心验收场景

### E2E-V2-001：发布与激活

1. 在 Control 创建资源、Workflow、Version 和 Application。
2. 生成 Bundle 并 Prepare。
3. 验证未 Activate 前生产入口不可调用该版本。
4. Activate 后验证 Runtime Head、Hash、Manifest、Resource Binding 和 Capability。
5. 验证首次 Activate 的 Head、Route 和最低 Admission Epoch 同事务可用；缺少 Key/Identity/Grant 时不得留下半激活 Head。
6. 重放同一 Outbox/Event，Runtime 数据不重复。
7. 激活第二版本并回滚第一版本，已有 Execution 不改变 Bundle，API Key/Revoke/Policy Epoch 不倒退。

### E2E-V2-002：控制面完全离线

1. 预先发布一个包含普通节点、Model、MCP、Wait 和 Artifact 的 Application。
2. 将 `web-console` 和 `platform-control` 的 `api/publisher/projector/retention` 全部 Role Deployment 缩容为零。
3. 通过 NetworkPolicy 切断 Runtime 到 Control Namespace。
4. 停止 Control MySQL 或切换为不可达 Endpoint；确认环境中本来就不存在 Control Redis。
5. 连续运行 API、Webhook、Schedule 和 Poll Invocation。
6. 验证全部 Invocation 创建真实 Execution并得到终态。
7. 验证 Session Context、SSE Cursor、Cancel、Wait Resume 和幂等重放。
8. 恢复控制面，验证事件补投且无重复 Message、Approval、Evaluation 或通知。

这是 V2 最核心的发布门禁，任何跳过都不能宣称控制/执行面分离完成。

V2-03 Run `20260813152731` 先完成 Gateway/Trigger/Session 基础切片，V2-04 补齐完整 Runtime Engine；V2-08A Run `20260818-final4` 已在最终本地三域拓扑复验控制面离线并关闭 `V2C-003` 的本地完成状态。

### E2E-V2-003：Runtime MySQL 故障

1. 创建在途 Execution。
2. 停止 Runtime MySQL。
3. 验证 Gateway 不接受无法持久化的新请求，不返回伪 `202`。
4. Worker/Coordinator Readiness 失败但进程不破坏本地数据。
5. 恢复数据库后验证 Lease/Reaper 收敛。
6. 验证无重复终态、无孤立 Invocation、无无条件重复副作用。

### E2E-V2-004：Runtime Redis 丢失和重建

1. 在 Outbox、Pending 和 Running 三个状态注入 Redis 停机。
2. 验证 Runtime MySQL 权威状态不丢失。
3. 重建空 Redis。
4. 验证 Consumer Group、Stream、Outbox 和 Quota Counter 自动重建。
5. 最终所有可重试执行收敛，终态只出现一次。

### E2E-V2-005：多副本竞争（done）

所有常驻服务至少两个副本，专项覆盖：

- Publisher 重复发送同一 Bundle。
- Gateway 并发相同 Idempotency Key。
- Trigger Worker 同时扫描相同 Schedule/Poll。
- Coordinator 同时处理相同 Command/Result。
- Runtime Worker 同时 Claim Outbox/Wait/Lease/Artifact。
- Control Pull Projector 同时处理相同 Event。
- Sandbox Reaper 同时发现相同 Sandbox。
- Trace Relay 同时看到相同 Outbox。

必须通过数据库断言证明业务事实只创建一次；允许传输层重复时必须证明 Receipt/Idempotency 收敛。

V2-06A Run `20260816-v206a-final6` 已完成上述竞争、Pod 强退、Owner/Fencing 接管和数据库唯一终态断言，证据见 [V2-06A 验收](evidence/v2-06.md)。

### E2E-V2-006：滚动扩缩容（done）

在持续发送 Invocation 和 SSE 连接时：

- 默认紧凑 Profile：`platform-control`、`runtime-gateway`、`workflow-runtime`、`workflow-worker` 和 `observability` 分别执行 2→N→2。
- 可选拆分 Profile：只对已达到拆分阈值的 Coordinator/后台 Role、Capability Worker Pool、Control Pull Projector 或 Trace Consumer 执行独立 1→N→1。
- `sandbox-manager` 启用时执行 2→4→2。

验证无请求静默丢失、SSE 可重连、Lease 可接管、队列最终清空、Quota Reservation 最终为零。

V2-06A Run `20260816-v206a-final6` 已使七类紧凑 Profile 工作负载完成真实 `2→4→2`、Gateway/Worker Drain、SSE `Last-Event-ID` 重连、滚动重启、PDB Eviction 和零残留断言。没有容量证据支持 Role 拆分，因此未启用可选拆分 Profile；容量门禁继续由 `V2S-006` 追踪。

### E2E-V2-007：OSS、ClickHouse、Vault、OpenSandbox

| 故障 | 断言 |
|---|---|
| OSS 暂停 | 需要 Artifact 的任务按策略失败/重试；小结果不被无关阻断 |
| ClickHouse 暂停 | Workflow 正常终止；Trace Outbox 积压，恢复后补投 |
| Vault 暂停 | Secret 节点不回退本地明文；非 Secret 节点可运行 |
| OpenSandbox 暂停 | Sandbox 节点明确失败/重试；普通 Worker Pool 不受影响 |

### E2E-V2-008：授权传播

1. 发布有效 Bundle并成功执行。
2. 撤销 Runtime Grant，等待 Runtime Inbox 应用。
3. 新 Execution 被拒绝；历史 Snapshot 保持可解释。
4. 已取得且仍在 Lease 内的 Handle 按冻结策略收敛。
5. 模拟 Control 完全断网，验证 Last Known Good。
6. 设置较短 `max_policy_staleness`，超时后新 Execution Fail Closed。

### E2E-V2-009：查询边界

- Control 运行列表通过 BFF 查询 Runtime Query API，不直连 Runtime MySQL，Control MySQL 中不存在 Execution/Invocation Summary 列表副本。
- Runtime 详情来自 Runtime Query API。
- Trace 来自 Observability API。
- 关闭 ClickHouse 后详情仍能显示 Runtime 终态。
- Runtime NetworkPolicy 中无法连接 Control MySQL。
- Platform API Credential 中不存在 Runtime MySQL 直连密码，BFF 只调用 Runtime API。
- BFF Delegation Token 不能跨 Tenant、跨 Scope、过期或重放查询 Runtime 对象。

### E2E-V2-010：产品能力等价回归

以 [V2 产品能力等价矩阵](99-traceability.md#2-产品能力等价矩阵) 为唯一清单，至少覆盖：

- Workflow 5.0 Start/End、Expression、Context CAS、IF/Switch/Merge/Loop 和固定版本 Composite。
- Model、MCP、Skill 递归依赖、RAG、Memory、Agent Budget 和 Sandbox。
- Session 固定 Bundle、显式升级、Message、正式 End Output 和 SSE 重连。
- Wait、Approval Decision、Resume、Checkpoint、Fork 和副作用确认。
- Dataset/Evaluation Profile 固定版本、批量 Case、Evaluator、报告和取消。
- Workflow Package、Runtime OSS 制品闭包、Artifact 授权和 Trace 跳转。
- 设计期资源授权会签、撤权、安全删除和双语控制面回归。

每项能力必须在 Control/Runtime 数据隔离开启的环境中运行，不能复用共享数据库 Fixture 伪造闭环。

### E2E-V2-011：模糊提交与乱序

在以下位置注入“服务端可能已成功、调用方未收到响应”的连接中断：

- Runtime 已保存 Prepare，但 Control Publisher 超时。
- Runtime 已切换 Head，但 Activate 响应丢失。
- Gateway 已提交 Invocation/Message/Command 事务，但 HTTP 连接断开。
- Coordinator 已提交 Result 和下游状态，但 Worker 未收到响应。
- Inbox 已应用事件，但发送方未收到 Receipt。
- Redis 已 `XADD`，但 Runtime Trace/Execution Outbox 尚未标记成功。
- 外部 Provider 已接受副作用，Worker 在记录结果前强退。

验证相同 Idempotency Key 重试得到同一业务结果；对无法通用 Exactly Once 的 Provider，必须保存风险状态、对账结果和人工补偿入口。另需乱序投递同一对象的不同 Version，验证旧状态不覆盖新 Epoch/Version；自包含状态允许 Version 跨越并直接收敛，不要求补齐不存在的通用 Sequence Gap。

### E2E-V2-012：滚动版本兼容

1. 部署当前版本和上一支持版本的 Gateway、Coordinator、Worker 和 Relay 混合集群。
2. 验证两代支持范围内的 Bundle、IR、Event 和 Internal API 可以滚动处理。
3. 先执行 Expand Migration，再滚动应用，最后执行 Contract 清理。
4. 验证旧 Pod 不会因新列/新事件崩溃，新 Pod 不领取未支持的旧/未来 Capability。
5. 对超出支持窗口的 Bundle 验证 Prepare/Rollback 被明确拒绝，而不是 Invocation 后失败。

## 3. 容量基线与发布阈值

V2 第一版必须记录以下容量结果：

- 并发 Invocation QPS 和 p95/p99 接受延迟。
- 100、500、1000 并发 Execution。
- 5000 Node Attempt 队列吞吐。
- 1000 并发 SSE 和重连风暴。
- 200 节点 Workflow。
- Model/Agent 和 Sandbox 独立 Worker Pool 背压。
- Control Pull Projector 和 Trace Consumer 最大可接受积压恢复时间；暂停两者期间 Runtime 当前状态查询与恢复仍正确。
- Runtime MySQL 连接数、锁等待、慢查询和 IOPS。
- Runtime Redis Pending、最老消息年龄和内存。
- 2 小时稳定性运行后的 Lease、Reservation、Outbox 和 Inbox 零漂移。

容量门禁必须记录硬件、Pod 副本、Pod 并发、中间件规格和外部 Provider 限额，不能只记录一个吞吐数字。

V2-00 必须在首次容量 Run 前冻结一份与测试环境规格绑定的数值阈值，至少包括：

- Invocation 非预期错误率、p95/p99 接受延迟和明确 Admission Reject 比例。
- SSE 建连/重连成功率、最大重放延迟和 Drain 时间。
- Ready Attempt/Outbox/Inbox/Trace 的最大消息年龄和故障恢复清空时间。
- Runtime MySQL 最大连接数、池等待、锁等待、死锁率、慢查询和 IOPS 安全水位。
- Runtime Redis 内存、Pending、Consumer Lag 和重建完成时间。
- Provider/Sandbox 隔离池的并发、排队超时和熔断恢复时间。
- 2 小时稳定性结束后 Lease、Reservation、Outbox、Inbox、Retention Hold 的允许残留量；业务类残留默认必须为零。

阈值尚未冻结或超过阈值时，容量场景只能记为数据采集，不能通过 V2-08 发布门禁。阈值变更需要记录原因、硬件差异和审批人，不能为了让失败 Run 通过而原地放宽。

## 4. 静态与契约门禁

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- 前端 lint、test、TypeScript 和 production build。
- Control/Runtime OpenAPI 无漂移。
- Bundle/Event JSON Schema 无漂移。
- Control/Runtime/ClickHouse 空库 Migration 测试。
- Kustomize/Profile 全组合渲染。
- 默认紧凑 Profile 常驻应用 Deployment 不超过 7 类；Role 拆分 Profile 必须关联容量或权限隔离证据，且复用原构建产物和版本。
- SQL 表归属扫描：Runtime 代码不得引用 Control 表，反之亦然。
- 环境变量扫描：Runtime 清单不得注入 Control MySQL Secret；任何 Control 清单均不得注入 Redis Secret。
- Control Redis 移除扫描：Control 清单、Secret、Settings、代码和 NetworkPolicy 中不存在 Redis 依赖，Control Pod 无法连接 Runtime Redis。
- Gateway Redis 专项扫描：`runtime-gateway` 只有 `sse_wakeup` 模块可使用 Runtime Redis；认证、Route、Invocation 接受、SQL 和幂等代码出现 Redis 直接依赖时失败。
- 数据库权限扫描：Observability Consumer 不得持有 Runtime MySQL Credential，各服务账号不能读写契约外表。
- NetworkPolicy 矩阵：Control/Runtime 数据端口越界必须失败，冻结的 Internal API 和 Runtime Provider 白名单必须成功。
- 后端/前端生产源文件不超过 2000 行；生成代码和 Migration 需在允许列表中显式说明。
- V2 产品能力矩阵的每一行都有自动化测试路径且无孤儿测试。
- 依赖扫描中不存在 Kafka/Pulsar/NATS Client、Broker 清单或 Event Sourcing Framework；Outbox/Event 不得成为 Execution 当前状态的唯一来源。
- Schema/API 扫描中不存在通用 Control Runtime Summary/Cost/Status 表、双路径列表开关或未使用的消息平台抽象；治理投影白名单只包含 Approval/Evaluation/Notification、Debug/Retention 操作结果、审计、告警和必要关联索引。
- 跨面 Event/Command Schema 默认携带自包含状态、终态结果或不可变对象引用，不存在未通过 ADR 的通用增量 Patch、连续 Aggregate Sequence 或 Gap Repair Framework。
- `git diff --check`。

## 5. 证据

每次 Run 保存：

- JUnit。
- Playwright HTML Report。
- Runtime/Control/Observability API 请求摘要。
- MySQL 权威状态断言。
- Redis Stream/Pending 摘要。
- ClickHouse Trace 数量和去重断言。
- Kubernetes Pod、ReplicaSet、Event、NetworkPolicy、PDB 和用户驱动副本变化状态；不要求 Agentx HPA。
- 故障开始/恢复时间线。
- Outbox/Inbox/Lease/Reservation 最终残留检查。

证据不得保存 API Key、Vault Token、Credential 明文、Webhook Secret 或完整用户输入。

## 6. 08A 与最终生产验收门禁

V2-08A 已通过本地功能、故障和性能回归，证据见 [V2-08A Run `20260818-final4`](evidence/v2-08.md)。这允许把 08A 标记为 `done`，但不等价于整个 V2-08 或 V2 生产完成。

只有满足以下全部条件才允许在 V2-08B 把整个 V2-08 标记为 `done`：

1. E2E-V2-001～012 全部 `failures=0`、`errors=0`、`skipped=0`。
2. 控制面离线期间持续 Invocation 没有非预期失败。
3. 所有常驻服务以多副本通过强退和竞争测试。
4. Runtime Redis 空库恢复成功。
5. Runtime MySQL 故障期间没有伪成功或不可解释状态。
6. ClickHouse 故障不影响 Runtime 终态。
7. NetworkPolicy 和 Secret 扫描证明 Runtime 不可访问 Control 数据库。
8. V1 共享 Schema、兼容层和旧部署 Profile 已删除。
9. [V2 架构与产品能力追踪矩阵](99-traceability.md)的两张矩阵都不存在 `planned`、`in_progress` 或 `blocked`。
10. 容量发布阈值已提前冻结且全部通过；只记录吞吐而没有通过标准不能标记完成。
