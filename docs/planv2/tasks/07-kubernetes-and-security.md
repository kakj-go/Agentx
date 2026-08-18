# V2-07 Kubernetes、运维与安全隔离任务清单

当前状态：`in_progress`。V2-07A 已完成代码与静态门禁，真实外部 TLS Kubernetes E2E 尚未执行；V2-07B 的强 RuntimeClass、Role 级权限拆分和最终供应链门禁继续延期。

本阶段把已经验证的代码边界固化为可安装、可升级、可恢复的 Kubernetes 边界。来源任务为 [V2K-001～006](../03-refactor-phases.md#9-v2-07-kubernetes运维和安全隔离)。

## 1. 进入条件

- V2-00～06 的数据所有权、Role、端口、Secret、连接预算和外部扩缩容指标契约稳定。
- 默认紧凑 Profile 已证明可以承载首期负载；任何 Role 拆分都有容量或权限证据。
- Control/Runtime/ClickHouse 独立 Migration Runner 和最小权限账号已通过。

## 2. 推荐批次

```text
K0 Profile Schema 和三域渲染
 → K1 ServiceAccount/Secret/NetworkPolicy
 → K2 Migration/Install/Upgrade Target
 → K3 PDB/探针/metrics 暴露/运维观测
 → K4 备份恢复演练
 → K5 Control/Runtime 独立升级与安全负向测试
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2K-001 | in_progress | V2S-005；V2S-006 延期 | Production Profile 已支持外部托管依赖、不可变镜像和三域独立 Target；14 场景 E2E 编排已冻结，等待真实 E2E | Profile Schema、紧凑 Profile | 静态全组合已渲染；真实外部 TLS Run 待完成 |
| V2K-002 | in_progress | V2A-009、V2D-007、V2K-001 | 已实现 restricted Pod 基线、默认拒绝和固定 CIDR Egress；强 RuntimeClass留给 07B | NetworkPolicy 矩阵和测试 Pod | 静态正负扫描通过；集群矩阵待完成 |
| V2K-003 | in_progress | V2D-004/005、V2K-001 | 已实现 existing Secret、工作负载级 Secret、CA 投影和双 `kid`；Role 级拆分及最终供应链留给 07B | Secret/ServiceAccount/IAM 清单和扫描 | 工作负载 Secret 静态隔离通过；Role 级门禁延期 |
| V2K-004 | in_progress | V2D-001～003、V2K-001～003 | 已实现独立 Target、唯一 Job、60 秒锁超时、协议窗口检查和 Contract 旧 ReplicaSet 前置检查 | Migration Target/Job/Runbook | 静态门禁通过；多 Job 集群竞争待完成 |
| V2K-005 | in_progress | V2K-001～004 | 已冻结 RPO/RTO、严格 Backup Receipt、恢复目标和 `Backup → Restore → Verify` 编排 | 备份恢复 Runbook 与演练证据 | Adapter/Manifest 正负测试通过；真实 PITR/恢复计时待完成 |
| V2K-006 | in_progress | V2K-001～005 | 已支持独立 Action、完整 Status、Release 状态和升级失败回滚编排 | 部署脚本、独立 Target 和升级 Suite | Target 静态隔离通过；持续 Invocation 集群升级待完成 |

## 4. 默认紧凑 Profile 约束

- 常驻应用类型不超过：`web-console`、`platform-control`、`runtime-gateway`、`workflow-runtime`、`workflow-worker`、可选 `sandbox-manager`、`observability`。
- `platform-control` 默认启动 `api,publisher,projector,retention`。
- `workflow-runtime` 默认启动 `coordinator,trigger,command,outbox,recovery,artifact,quota,trace-relay`。
- `observability` 默认启动 `trace-consumer,query`。
- Role 拆分只改变 Deployment、副本、权限或外部扩缩容边界；不新增镜像、数据所有权、业务 API 或版本线。
- Agentx 不安装 Prometheus、Prometheus Adapter 或 Metrics Server，不创建 HPA/KEDA；用户平台负责抓取 `9092 /metrics`、告警和扩缩容。
- `replicas` 只在首次安装时生效，Upgrade/Rollback 保留当前副本数；`maxReplicas` 只作为连接池和外部依赖容量预算。

## 5. 安全与运维负向测试

- Runtime Pod 读取 Control MySQL Secret、访问 Control DB 端口或解析 Control Service DNS。
- Control Pod 获取任何 Redis Secret或连接 Runtime Redis。
- Observability Pod 获取 Runtime MySQL Credential。
- Worker 覆盖 Control OSS Object，Control 账号覆盖 Runtime Artifact。
- 错误 Role/Audience/Scope/过期 Service JWT 调用 Internal API。
- BFF Delegation Token 跨租户、跨 Execution/Session/Application 或过期重放。
- 两个 Migration Job 同时操作同一数据库。
- Contract Migration 早于旧 Pod 下线。

## 6. 阶段门禁

- 可以独立安装、升级、缩容、查看和恢复 Control 或 Runtime Target。
- Secret/Env/NetworkPolicy/数据库权限共同证明跨面数据端口不可达，而不只依赖应用自律。
- 默认 Profile 不超过 7 类常驻应用 Deployment；拆分 Profile 有可复现证据。
- 三套 Migration 可独立锁定和运行，Expand/Contract 滚动验证通过。
- 备份恢复达到冻结 RPO/RTO；Control 升级期间 Runtime 持续探针无非预期失败。

阶段证据摘要保存为 `docs/planv2/evidence/v2-07.md`，至少包含 NetworkPolicy 正负矩阵、Secret 扫描和恢复时间线。
