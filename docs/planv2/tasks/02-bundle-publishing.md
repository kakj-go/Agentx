# V2-02 Execution Spec Bundle 发布任务清单

本阶段先打通最小但真实的垂直切片，证明生产执行不再依赖控制面。来源任务为 [V2P-001～009](../03-refactor-phases.md#4-v2-02-runtime-bundle-发布链路)。

## 1. 进入条件

- V2-01 的两个 MySQL、Runtime Redis、OSS 域、账号和 NetworkPolicy 已通过。
- Bundle、Admission、Inbox/Outbox、Internal API 和版本窗口已经冻结。
- 已选择一个只包含基础节点、确定性输入输出且不依赖旧 Control SQL 的验收 Workflow。

## 2. 推荐批次

```text
P0 Bundle Builder 和对象闭包
 → P1 Control Publish Attempt/Outbox/Publisher
 → P2 Runtime Prepare/Inbox/本地投影
 → P3 Activate/Head CAS/Rollback/Disable
 → P4 Gateway→Coordinator→Worker→Query 基础切片
 → P5 UI、引用和 GC
 → P6 控制面离线门禁
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2P-001 | done | V2D-001～003 | 从固定 Version 解析 Definition、IR、Manifest、Resource、Composite 和策略闭包；规范化编码、签名并生成 Content Hash | Bundle Builder、Fixture 和 Schema | 相同输入/编译器版本得到相同 Hash；缺失/循环/可变 Head 引用在发布时失败 |
| V2P-007 | done | V2P-001、V2D-005 | 生成 Object Manifest；按 Tenant+Hash 复制到 Runtime OSS；逐项验证 Hash/Size/Media Type；失败对象进入补偿清理 | Runtime Object Publisher 和 Manifest | Runtime 断开 Control OSS 后仍可读取全部依赖；越域和篡改对象被拒绝 |
| V2P-002 | done | V2P-001、007 | Control 事务创建 Publish Attempt、Bundle 和 Outbox；Publisher 多副本 Claim，使用稳定 Idempotency Key 调 Runtime | `platform-control --role=publisher` | 重复领取、Pod 强退、HTTP 超时和结果未知均收敛到同一 Attempt |
| V2P-003 | done | V2P-002、V2D-002 | 实现独立 Runtime Publish Internal API：Prepare/Activate/Rollback/Disable；验证 JWT、Scope、Schema、签名、Hash、Capability 和 Inbox | Internal API、Runtime Inbox/Receipt | 重放同一请求返回既有结果；相同事件不同 Hash、旧 Epoch、错误 Audience 和未知版本失败 |
| V2P-004 | done | V2P-003 | 在 Runtime 建立 Bundle、Route、Head、Key/Identity、Trigger、Grant 和最低 Admission Epoch 投影；Activation Manifest 原子应用 | Runtime 发布 Repository | 首次激活缺任一 Admission 前置时不留下半激活 Head；生产读取只查 Runtime |
| V2P-006 | done | V2P-004 | 将基础 Execution 创建改为只接收 `bundle_id`；删除对应 Version/Catalog/Grant 控制查询和过渡 Adapter | Bundle 驱动的基础 Snapshot | 静态 SQL/Env 扫描无 Control 表和 DSN；关闭 Control 仍可创建 Execution |
| V2P-008 | done | V2P-004、006 | 打通 Runtime Gateway→Invocation→Coordinator→Worker→Runtime Query；状态与 Outbox 同事务，Redis 只做派发 | 基础 Workflow 垂直切片 | Control 全部缩容/断网后连续调用得到真实终态、输出和查询结果 |
| V2P-005 | done | V2P-002～004 | 提供 Publish Attempt 查询和 UI；显示 building/prepared/active/rejected、结构化失败、重试和回滚兼容性 | 控制 API/OpenAPI/生成 Client/UI | UI 能区分 Prepare 与 Activate；未激活版本不可生产调用；失败可定位且不暴露 Secret |
| V2P-009 | done | V2P-004、008 | 建立通用 Bundle/Object Reference 与 Retention Hold 存储，并接入 `active_execution`；实现 Bundle 和未绑定 Ready 对象的幂等 GC 骨架 | Reference Repository、Mark/Sweep 任务 | Active/Prepared/Head/Reference/Hold 阻止删除；失败可重试，删除后原幂等上传可恢复 |

## 4. 必须覆盖的模糊提交场景

- Runtime 已保存 Prepare，但 Publisher 未收到响应。
- Runtime 已原子切换 Head，但 Activate 响应丢失。
- Publisher 在外部调用后、标记 Outbox 前强退。
- 相同 Idempotency Key 被两个 Publisher 副本同时发送。
- 回滚请求乱序到达，旧 Head Version 或较低 Activation Sequence 被拒绝。
- Runtime OSS 已复制部分对象，但 Bundle Prepare 失败；孤儿对象最终被补偿或由 GC 清除。
- Ready 正式对象尚未绑定 Bundle 时与 Prepare 并发；对象锁和 `ready/deleting` 状态必须保证只有一方推进。

## 5. 阶段门禁

- Publish、Prepare、Activate、Rollback、Disable、重复和崩溃测试通过。
- 未激活 Bundle 永不接收生产请求，激活切换不包含大对象复制。
- Bundle 回滚只改变新 Execution 使用的 Spec Head，不回滚 Admission State。
- 最小 Workflow 在 Web、Platform API、Control 后台 Role、Control MySQL 和 Control OSS 元数据均不可用时完整执行。
- 新链路通过后，对应运行时控制配置拼装和共享数据库查询已经删除。

阶段证据摘要保存为 `docs/planv2/evidence/v2-02.md`，并至少包含 E2E-V2-001 和基础版 E2E-V2-002 结果。

## 6. 完成证据

`V2P-001～009` 的实现、专项测试、最终 Kubernetes Run、全量门禁和未覆盖能力边界统一记录在 [V2-02 验收证据](../evidence/v2-02.md)。最终 Run 为 `20260813v202p`：E2E-V2-001 覆盖发布、Activate Receipt 提交后的 Publisher 强退接管、对象复制、第二版本和回滚；基础版 E2E-V2-002 覆盖控制面离线后的 10 次真实 `no_op` Execution 与 Runtime Query。11 张增量表所有权、Gateway 无 Redis、Disable Receipt 和对象级 GC 均有专项门禁。完整 Model/MCP/Wait/Session/SSE 等离线能力，以及 Session/Wait/Checkpoint/Fork 的完整引用生命周期，仍属于 V2-03/V2-04。
