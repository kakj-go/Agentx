# V2-05 查询、治理投影与可观测面任务清单

本阶段建立清晰的三类查询来源：Runtime 当前状态、Control 治理投影和 ClickHouse Trace。来源任务为 [V2Q-001～007](../03-refactor-phases.md#7-v2-05投影查询和可观测面)。

## 1. 进入条件

- 对应产品能力的 Runtime 权威状态已经在 V2-03/V2-04 迁移完成。
- Runtime 当前状态不依赖异步消费自身 Event 才能生成。
- Runtime→Control Event Envelope、Cursor/Receipt、Snapshot Export 和 Delegation Token 已冻结。

## 2. 查询边界

| 用户需求 | 唯一权威路径 | 降级行为 |
|---|---|---|
| Workflow、资源、版本、发布配置 | Browser → Platform API → Control MySQL/OSS | Control 不可用时不可查询或修改 |
| Invocation/Execution 列表与详情 | Browser → Platform BFF → Runtime Query API → Runtime MySQL | Runtime 不可用时明确失败，不展示 Control 陈旧副本 |
| 生产调用方查询/SSE | Client → Runtime Gateway/Query → Runtime MySQL/OSS | Redis 仅影响唤醒，不丢 Cursor |
| Approval/Evaluation/Notification 等治理结果 | Platform API → Control 治理投影 | 重建时返回 projection status/cursor |
| Trace、成本、错误和长时间聚合 | Browser → Observability API → ClickHouse | Trace 延迟不改变 Runtime 终态 |

## 3. 推荐批次

```text
Q0 Runtime 状态/索引事务化
 → Q1 Runtime Query List/Detail 和 BFF
 → Q2 Runtime Event/Snapshot Export + Control Pull
 → Q3 治理投影重建
 → Q4 Trace Relay/Consumer
 → Q5 Observability Query 和页面降级
```

## 4. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2Q-001 | done | 已迁移的 V2G/V2R 状态 | Invocation、Message、Approval、Evaluation、Execution 当前字段和必要索引与领域状态、Outbox 同事务更新；删除异步 Runtime Projector | Runtime Query Schema/Repository | 停止所有 Consumer 后，查询、恢复和状态推进仍正确；Event 删除不影响当前状态 |
| V2Q-003 | done | V2Q-001 | 实现 Execution/Invocation 列表的 Tenant/Scope 校验、稳定排序、强一致分页快照、覆盖索引和查询预算 | Runtime List Query API/OpenAPI | 并发写入时不重复/漏页；Control MySQL 无 Summary 表；越权和高成本查询被拒绝 |
| V2Q-004 | done | V2Q-001、003 | Platform BFF 校验 Control IAM，签发短期 Delegation Token 调 Runtime Detail/List；Runtime 二次校验并审计 | BFF Client、Delegation Token 和详情 API | Platform 无 Runtime DB 密码；Token 跨 Tenant/Scope、过期或重放失败；无任意租户工作负载特权 |
| V2Q-002 | done | V2Q-001、V2A-002 | Runtime 以全局单调 Cursor 暴露受限 Event Export/Long Poll 和 Snapshot Export；Control Projector 多副本拉取，Receipt/投影/Cursor 同事务 | Runtime Export API、Control Pull Projector | Runtime 零反向调用；Control 不连 Runtime DB；离线恢复无重复通知/审批/评测；旧 Version 不覆盖新值 |
| V2Q-007 | done | V2Q-002 | 为 Approval/Evaluation/Notification、Debug/Retention 结果和必要审计/告警实现治理 Snapshot+增量重建；暴露重建状态 | 治理投影和重建任务 | Cursor 超保留窗仍能从 Snapshot 恢复；重建时不把缺失显示为不存在；不复制通用列表/成本/Status |
| V2Q-005 | done | V2Q-001、V2R-004 | Runtime `trace-relay` Claim MySQL Trace Outbox 并 XADD；Observability Consumer Group 批写 ClickHouse，以稳定 Event ID 去重 | Trace Relay、Consumer、CH Repository | Consumer 无 Runtime DB 凭据；XADD/标记/ACK 任意点强退允许重复但不丢失；CH 停机后补投 |
| V2Q-006 | done | V2Q-005 | 提供 Trace/Cost/Error/Aggregation API；限制时间范围、行数、Cursor、Tenant 并发、Query ID 和取消；前端组合 Runtime 终态与 Trace | Observability API/OpenAPI/UI | 租户隔离、超时/取消、脱敏和重查询限流通过；CH 故障时页面仍显示权威终态并标记延迟 |

## 5. 一致性和保留测试

- Runtime Event Export 使用自包含状态/终态，不要求通用 Aggregate Gap Repair。
- 相同 `event_id` 和不同 `content_hash` 必须进入安全错误/人工处置。
- Control 只有在 Receipt、投影和 Cursor 同事务提交后才确认消费。
- Integration Event Log 保留必须覆盖 V2-00 冻结的最大控制面离线时间；超窗使用 Snapshot，不开放数据库直连。
- Runtime List 和 Detail 使用同一权威数据源；禁止页面在 Control Summary 与 Runtime Detail 之间形成暂时矛盾。
- ClickHouse/Event Consumer 停止期间，Runtime 当前状态、查询和恢复仍必须完全正确。

## 6. 阶段门禁

- Control 配置、Runtime 当前状态和 Observability Trace 三类查询来源无双路径。
- Control MySQL 不存在通用 Execution/Invocation Summary、Cost 或 Runtime Status 副本。
- Platform API 没有 Runtime MySQL Credential，Observability 没有 Runtime MySQL Credential。
- Control Projector 停机、重复、乱序、Cursor 超窗和多副本竞争测试通过。
- ClickHouse 故障不影响 Workflow 终态；恢复后 Trace 无丢失且可去重。
- UI 对 Trace 延迟、治理投影重建和 Runtime Query 不可用有明确且一致的状态。

阶段证据摘要保存为 `docs/planv2/evidence/v2-05.md`，至少包含 E2E-V2-009 及 ClickHouse 故障结果。

最终证据为 [V2-05 Run `20260815112040`](../evidence/v2-05.md)：12 个 V2-05 场景、保留执行的 V2-04 12 场景基线和 `E2E-V2-009` 全部通过。
