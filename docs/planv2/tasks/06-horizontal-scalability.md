# V2-06 全服务横向扩展任务清单

本阶段不首次发明并发协议，而是审计所有已实现 Role，并用多副本、强退、滚动扩缩容和容量测试证明正确性。来源任务为 [V2S-001～006](../03-refactor-phases.md#8-v2-06全服务横向扩展)。当前阶段状态为 `in_progress`：V2-06A 已完成 `V2S-001～005`；V2-06B 的压力、容量、公平性和两小时稳定性明确延期，`V2S-006` 保持 `planned`，不作为 V2-07A 的门禁。

## 1. 进入条件

- V2-03～05 的 Runtime、Query、Event 和 Trace 主链路已经完成。
- 所有后台 Role 已使用 V2A-008 公共 Claim/Lease/Fencing 或明确的 Consumer Group/Leader Lease。
- V2-00 已冻结与测试环境规格绑定的容量、积压、恢复、SSE 和残留阈值。

## 2. 推荐批次

```text
06A: S0 全仓 Claim/Lease/Loop 审计
  → S1 Trigger/Coordinator/后台 Role 竞争
  → S2 Sandbox/Projector/Trace 专项竞争
  → S3 Readiness/Drain/PDB/指标契约
06B: S4 背压、公平性和连接预算
  → S5 容量、稳定性和故障恢复 Run
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2S-001 | done（[06A 证据](../evidence/v2-06.md)） | V2A-008、V2G-004、V2R-004、V2Q-002/005 | 扫描全部常驻循环和数据库 Claim；核对数据库时间、Owner、Fencing、批次、Retry、索引、外部 I/O 边界和过期接管 | Claim/Lease 审计目录和修复 | 无私有无锁扫描、Pod 本地过期时间或持事务执行外部 I/O；旧 Token 写入均被拒绝 |
| V2S-002 | done（[06A 证据](../evidence/v2-06.md)） | V2S-001 | 两副本以上并发 Trigger、Command、Outbox、Recovery、Wait、Artifact；在 Claim/外部调用/提交各阶段强退 | 竞争与故障注入 Suite | Schedule/Poll、状态转换和 Artifact 只产生一个业务结果；未知结果有对账或补偿 |
| V2S-003 | done（[06A 证据](../evidence/v2-06.md)） | V2S-001、V2R-009 | Sandbox Reaper 先 CAS 到 terminating+Lease，再调用 Provider；创建/恢复/终止使用稳定 Key 和 Label 对账 | Sandbox 多 Manager Suite | 多 Manager 只终止一次或收敛到一次终态；强退不留下不可解释孤儿 |
| V2S-004 | done（[06A 证据](../evidence/v2-06.md)） | V2S-001、V2Q-002/005 | 多副本 Control Pull Projector、Runtime Trace Relay 和 Trace Consumer 竞争；优化 Cursor 分片、Outbox Claim 和批量 | Projector/Trace 竞争 Suite | 无热点全表扫描、重复投影或 Trace 丢失；Runtime 当前状态不依赖这些 Role |
| V2S-005 | done（[06A 证据](../evidence/v2-06.md)） | V2S-002～004 | 为常驻 Deployment 配置 Readiness/Liveness、preStop、Drain、PDB 和稳定 `9092 /metrics`；扩缩容由用户平台实施 | PDB/Profile、指标契约和手工滚动扩缩容测试 | 用户执行 `2→4→2` 无静默丢失；SSE 重连；剩余 Lease 到期可接管；不靠 Sticky Session；Agentx 不创建 HPA |
| V2S-006 | planned | V2S-005 | 建立 MySQL/Redis/OSS/CH/Provider 全局预算；按 Tenant/Capability/Provider 公平限流；Gateway Admission、Retry-After 和熔断 | 容量脚本、背压规则和基线报告 | 过载明确拒绝而非无限接受；热租户不耗尽全局资源；达到冻结阈值且 2 小时残留合格 |

## 4. 必测副本矩阵

| 构建产物/Role | 最低副本 | 主要竞争点 | 强退位置 |
|---|---:|---|---|
| `platform-control` api/publisher/projector/retention | 2 | Outbox Claim、Cursor/Receipt、Retention Plan | Runtime 响应前后、Cursor 提交前后 |
| `runtime-gateway` | 2 | Idempotency、SSE Cursor、Head/Key 缓存版本 | Invocation 提交后、SSE Drain 中 |
| `workflow-runtime` coordinator/background roles | 2 | 状态版本、Command/Outbox/Wait/Lease | Claim 后、Result 提交后、Redis 发布后 |
| `workflow-worker` | 2/每 capability | Redis 消费、Attempt Lease、Provider 副作用 | Provider 接受后、Report Result 前后 |
| `sandbox-manager` | 2（启用时） | Sandbox Lease/Reaper | Create/Terminate 结果未知 |
| `observability` consumer/query | 2 | Consumer Group、CH 批写去重、查询预算 | CH 写入后、ACK 前 |

## 5. 容量 Run 最小集合

以下内容属于 V2-06B，尚未执行，不能使用 06A 的历史功能性扩缩容 Run 替代：

- Invocation QPS 与 p95/p99 接受延迟。
- 100/500/1000 并发 Execution、5000 Attempt 队列、1000 SSE 和重连风暴。
- 200 节点 Workflow；Model/Agent/Sandbox 分池背压。
- Projector/Trace Consumer 暂停后的积压年龄与恢复清空时间。
- Runtime MySQL 连接、池等待、锁等待、死锁、慢查询和 IOPS。
- Redis 内存、Pending、Consumer Lag 和空库重建时间。
- 2 小时稳定性结束后的 Lease、Reservation、Outbox、Inbox 和 Hold 残留。

所有结果必须同时记录硬件、中间件规格、Pod 副本/并发和 Provider 限额。没有提前冻结阈值的 Run 只能作为数据采集，不能通过门禁。

## 6. 06A 已通过门禁

- 七类常驻 Deployment 以两个副本启动并完成竞争、强退、Drain 和滚动发布测试。
- 紧凑 Profile 和七个 PDB 完成真实 `2→4→2`；SSE 按 MySQL Cursor 跨 Pod 重连。该历史 Run 曾使用七个 HPA，但当前部署契约已改为用户或外部平台自行扩缩容。
- Claim 审计、Migration `0006`、应用 metrics 端点、NetworkPolicy 和最大副本连接预算门禁通过。Agentx 当前不安装 Prometheus、Adapter、Metrics Server，也不创建 HPA。
- `E2E-V2-005`、`E2E-V2-006` 已由 [Run `20260816-v206a-final6`](../evidence/v2-06.md) 关闭。

## 7. V2-06 完整阶段门禁

- 所有常驻 Deployment 以至少两个副本完成竞争、强退和滚动发布测试。
- 代码、配置和运维手册没有以 `replicas: 1` 作为正确性前提。
- 默认紧凑 Profile 完成 2→N→2；Role 拆分仅在证据达到阈值时启用。
- 连接池总和不超过各基础设施预算；过载有稳定错误码和 `Retry-After`。
- E2E-V2-005、006 已通过；容量/公平性/背压和真实两小时稳定性门禁仍必须由 V2-06B 通过。

06A 阶段证据摘要保存为 `docs/planv2/evidence/v2-06.md`，原始证据保存到 `artifacts/v2/20260816-v206a-final6/v2-06/06a/`。V2-06B 必须使用独立 Run 保存容量原始数据。
