# V2-03 Runtime Gateway 与生产业务状态任务清单

本阶段把全部生产入口、会话、调用和触发状态迁到 Runtime。来源任务为 [V2G-001～008](../03-refactor-phases.md#5-v2-03-runtime-gateway-和生产业务状态)。

## 1. 进入条件

- V2P-008 基础垂直切片和控制面离线验证通过。
- Runtime Bundle、Head、Admission Epoch、Inbox 和 Query 骨架可复用。
- 每迁移一种入口，都有旧路径删除点和对应产品回归用例。

## 2. 推荐批次

```text
G0 Route/API Key/Admission
 → G1 Invocation/Session/Message 幂等事务
 → G2 Runtime Query/SSE/Cancel/Resume
 → G3 Schedule/Poll/Lifecycle Trigger
 → G4 独立 Ingress 和旧 Control 查询删除
 → G5 模糊提交、时间语义和离线 E2E
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2G-001 | done | V2P-003～004 | Runtime 持有 Application Route、Head、API Key Hash、Webhook Secret Reference、JWT Policy、启停和单调版本缓存 | Runtime Gateway Repository/Auth | Control DB 离线仍可鉴权路由；Key 轮换/撤销、应用停用和旧版本乱序不倒退 |
| V2G-002 | done | V2G-001 | 将 Session、Context、Message、Message Part、Invocation、Idempotency Record 迁入 Runtime；同事务创建调用和 Execution Command | Runtime Schema、Repository 和 API | 并发相同 Key 只有一个业务事实；Session 固定 Bundle；失败不留孤立 Message/Invocation |
| V2G-007 | done | V2G-002 | 为 Invocation/Message/Command 定义提交后响应丢失协议、结果查询和冲突语义 | 幂等结果 API 和故障 Fixture | 数据库已提交但 HTTP 断开后，同 Key 重试返回原 ID/状态，不创建第二条记录 |
| V2G-003 | done | V2G-002、007 | Runtime Query 返回 Invocation/Execution/Output；SSE Cursor 写 Runtime MySQL，Redis 只唤醒；实现 Cancel 和 Wait Resume | Query/SSE API、Cursor 和 Delegation 校验 | 任意 Gateway Pod 可用 `Last-Event-ID` 接续；Redis 丢通知后仍可从 DB 重放 |
| V2G-004 | done | V2G-001、002 | 提取 `workflow-runtime --role=trigger`；Schedule/Poll/Lifecycle 使用两步 Claim、稳定业务幂等键和 Runtime Command | Trigger Role 和多副本测试 | 两副本扫描不重复业务执行；默认紧凑 Profile 可合并 Role，不依赖单副本 |
| V2G-008 | done | V2G-004 | 冻结 TZ 数据版本、Misfire、Catch-up、最大追赶、Poll Cursor、时钟跳变和禁用后在途规则 | Trigger 时间契约和 Fixture | 迟到、DST/时钟跳变、响应未知、Cursor 重放和启停竞争均收敛 |
| V2G-005 | done | V2G-001～004 | 建立独立 Runtime Ingress/域名和限流；生产请求不经过 Web Nginx 或 Platform API | Runtime Ingress/Profile | Web/Platform 全部零副本时 API、Webhook、SSE 仍可用；控制入口不能调用生产管理端口 |
| V2G-006 | done | V2G-001～005 | 删除 Gateway 中 IAM/Department/Control Deployment SQL、旧 Web 代理和控制面调用代理 | 删除清单和静态扫描规则 | Runtime Gateway 二进制、Env、SQL 和清单不含 Control DB/Service 依赖 |

## 4. 入口逐项关闭标准

每种入口必须分别完成以下闭环，不能用 API 调用成功代替其余入口：

| 入口 | 幂等来源 | 必测故障 |
|---|---|---|
| API/Application | 客户端 Idempotency Key | 提交后断连、Key 轮换、Head 切换 |
| Webhook | Provider Event ID/签名窗口 | 重放、乱序、Secret 轮换 |
| Schedule | Trigger ID + 计划时刻 | Misfire、追赶、双副本 Claim |
| Poll | Provider Cursor/Event ID/响应 Hash | 响应未知、Cursor 重放、Provider 超时 |
| Lifecycle | Binding Revision + Operation | Activate/Deactivate 并发、旧 Revision |
| SSE | 持久 Cursor/Last-Event-ID | Gateway 强退、Redis 丢失、滚动缩容 |

## 5. 阶段门禁

- API、Webhook、Schedule、Poll、Lifecycle、SSE、Cancel 和 Wait Resume 只使用 Runtime 依赖。
- Runtime MySQL 不可用时 Gateway 明确拒绝，绝不返回无法解释的成功或 `202`。
- Gateway 无 Sticky Session，至少两副本通过并发幂等、SSE 重连和 Drain。
- 生产域名不经过 Web Console/Platform API，控制面完全离线 E2E 通过。
- 对应旧 Control SQL、代理入口和后台 Trigger Loop 已删除。

阶段证据摘要保存为 `docs/planv2/evidence/v2-03.md`，原始结果保存到 `artifacts/v2/<run-id>/v2-03/`。

最终 Run `20260813152731` 已完成上述门禁，详见 [V2-03 验收证据](../evidence/v2-03.md)。Wait Resume 在本阶段只承诺鉴权、幂等和唯一 Command 落库；完整 Wait/Approval 状态机、Model/MCP 与 Checkpoint/Fork 继续由 V2-04 跟踪。
