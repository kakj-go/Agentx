# V2-04 Runtime Engine 数据独立任务清单

本阶段把现有 Runtime 产品能力逐项迁移到 Bundle、Runtime MySQL/Redis/OSS、Vault 和 OpenSandbox 上。来源任务为 [V2R-001～015](../03-refactor-phases.md#6-v2-04-runtime-engine-数据独立)。

## 1. 实施原则

- 按产品能力做垂直切片：Contracts/Bundle → Runtime Schema → Coordinator/Worker → Query/Event → E2E → 删除旧路径。
- 不能先批量删除共享实现，再等待阶段末补回全部能力。
- Execution 当前状态和必要查询索引在领域事务内同步提交，Event 不是状态权威。
- Worker 每次执行前必须取得 Attempt Lease；结果携带 `attempt_id + lease_token + result_hash`。
- 每完成一类能力，立即更新 V2 产品能力矩阵和对应回归证据。

## 2. 推荐批次

```text
R0 Bundle Snapshot、状态机、后台 Role、Worker Capability
 → R1 授权、资源、Credential、Redis 重建
 → R2 Agent/Sandbox
 → R3 Wait/Approval/Checkpoint/Fork
 → R4 Evaluation/Composite/Debug Work Package
 → R5 Quota/Retention/GC
 → R6 全产品能力离线回归
```

R1～R4 可以在 R0 稳定后按能力并行，但共享 Contracts、Claim/Lease、Runtime Call 和 Query 骨架必须复用，不能各自实现私有协议。

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2R-001 | done | V2P-004、006 | Execution 创建只读取已激活 Bundle，固定 Bundle/Admission Epoch 并生成 Snapshot | Runtime Snapshot Builder | 不读取 Workflow Version/Catalog；历史 Execution 可解释；Head 切换不修改在途执行 |
| V2R-005 | done | V2R-001 | 迁移 Item、Lineage、Expression、Context CAS、Attempt、Delivery、IF/Switch/Merge/Loop 和基础节点 | Runtime 状态机内核和 Fixture | Workflow 5.0 基础语义、重复 Delivery、冲突 CAS、循环预算和正式 End Output 通过 |
| V2R-004 | done | V2R-001、005 | 提取 command/outbox/recovery/artifact/quota Role；每个扫描使用公共 Claim/Lease/Fencing | `workflow-runtime` 多 Role 启动和测试 | 两副本竞争、过期接管、强退和 Drain 收敛；默认可合并 Deployment |
| V2R-006 | done | V2R-001、004 | Worker 按 Protocol/IR/Compiler/Capability 过滤后 Claim Attempt；通用池支持逻辑 capability | Capability Registry、兼容矩阵和外部扩缩容指标 | 旧/不兼容 Worker 不领取；Redis 重复消息不重复执行已完成 Attempt |
| V2R-002 | done | V2R-001 | Runtime 保存 Service Identity、Grant、Resource State、Policy Epoch 和 `max_policy_staleness`；新 Execution/需重授权 Attempt 本地校验 | Runtime Authorizer | 撤权传播后拒绝；LKG 和 Fail Closed 策略按租户生效；旧 Epoch 不能覆盖新值 |
| V2R-003 | done | V2R-002 | Resolver 只使用精确 Runtime Binding、Runtime Object 和 Vault Reference；Handle 短期且绑定 Execution/Attempt/Operation | Resource/Credential Resolver | Secret 无 DB 明文或 Control 回退；Handle 过期、撤销、跨执行重放失败 |
| V2R-007 | done | V2R-004、006 | 从 Runtime MySQL Outbox 重投 Task/通知；重建 Stream/Consumer Group/Pending 和 Quota Counter 校准 | Redis Bootstrap/Reconciler | 在 Outbox/Pending/Running 阶段清空 Redis，执行最终收敛且终态唯一 |
| V2R-008 | done | V2R-002、003、006 | 迁移 Model、MCP、RAG、Memory、Skill、Credential Runtime Call；解析精确版本和递归依赖 | 统一 Resource Runtime Adapter | Control 离线运行；授权缺口前置失败；调用、成本、错误和部分结果可追踪 |
| V2R-009 | done | V2R-003、006、008 | 迁移 Agent State/Iteration/Budget/Tool Loop；Sandbox 使用 Lease、短凭据、取消和 Provider 对账 | Agent Runtime、Sandbox Adapter/Manager | 预算、循环检测、恢复、TTL、网络、Secret、强退和孤儿回收通过 |
| V2R-010 | done | V2R-001、002、004 | Runtime 持有 Wait/Approval Task/Resume Token；Control Decision Command 带 Task Version，Runtime CAS 唯一终态 | Approval/Wait Runtime | 并发决定唯一应用、Receipt 重放、正确端口恢复和 Resume 不重复均通过 Runtime Slice 与 Kubernetes 故障矩阵 |
| V2R-011 | done | V2R-001、003、004 | Checkpoint/Artifact/Lineage/Fork Source 使用显式 Bundle Reference；部分重跑需副作用确认 | Checkpoint/Fork Runtime | 原执行不变；Fork 固定来源 Bundle；被引用 Bundle/Object 不被 GC |
| V2R-012 | done | V2R-001、003、004、008 | Control 生成固定 Dataset/Profile/Case 的 Evaluation Work Package；Runtime 持有 Case/Evaluator Execution 和取消状态 | Evaluation Runtime/Event | 批量真实执行、取消、规则、成本和 Runtime 事件通过；治理投影与 Trace 查询继续由 V2-05 追踪 |
| V2R-013 | done | V2R-001、005、008 | Bundle 包含固定 Composite/Sub-workflow Version、IR、对象闭包和递归检测结果 | Composite Runtime | Control 离线执行多层 Composite；循环和未固定子版本在发布时拒绝 |
| V2R-014 | done | V2R-001、003、005 | Control 用同一 Builder 生成带 Draft Revision/Overlay/TTL/Purpose 的 Debug Work Package，进入隔离 Queue/Quota | Debug Runtime 和结果 Event | 不创建生产 Head；不回读 Control；到期回收；Runtime 生成真实节点结果 |
| V2R-015 | done | V2R-001～014 | 实现 Admission Reservation、Usage Ledger、Retention Plan/Hold、Bundle/Object Mark-Sweep 和删除残留报告 | Quota/Retention Runtime | 并发/失败恢复零漂移；Hold 保护引用；跨库预检超时 Fail Closed；删除可重试 |

## 4. 每个产品切片的固定实施顺序

1. 在产品矩阵确认 Control 权威、Runtime 权威和跨面契约。
2. 扩展 Bundle/Work Package/Admission Schema，并补版本兼容 Fixture。
3. 扩展 Runtime Migration、Repository 和状态约束。
4. 实现 Coordinator/Worker/Manager 路径及幂等、Lease、响应未知处理。
5. 同事务维护 Runtime 当前状态、必要 Query Index 和 Outbox。
6. 增加 Runtime Query、Control 治理 Event 或 Trace；没有消费者也能继续执行和查询。
7. 在控制面网络断开的临时 Namespace 运行产品 E2E。
8. 删除该能力旧 Control SQL、共享表读取、过渡 Adapter 和双模配置。
9. 更新追踪矩阵、任务状态和证据。

## 5. 阶段门禁

- Coordinator、Workflow Runtime、Worker 和 Sandbox Manager 只持有 Runtime 数据域及必要外部 Provider 权限。
- Control NetworkPolicy 完全断开时，产品矩阵中的所有 Runtime 能力逐项通过，不以基础 Workflow 代替。
- Redis 丢失、Worker/Coordinator 强退、重复结果、Lease 过期和模糊提交均收敛。
- Approval、Evaluation、Debug、Fork、Composite、Agent 和资源 Runtime 有各自真实 E2E。
- 所有已替代的共享数据库读取、Runtime Projector 依赖和旧内部协议已经删除。

阶段已通过 Run `20260815013106` 的十二场景 Kubernetes E2E 与最终全量门禁；证据摘要保存于 [V2-04 验收证据](../evidence/v2-04.md)，并逐项链接 [V2 产品能力等价矩阵](../99-traceability.md#2-产品能力等价矩阵)。
