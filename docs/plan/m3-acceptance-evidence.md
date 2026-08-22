# M3 验收证据

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；请使用 `agentx-deploy`、`agentx-check` 与 `pytest tests/e2e`。

状态：`done`。验收日期：2026-08-03。

M3 已完成阶段 05 至 07 的外围控制面和运行查询链路。Workflow Scheduler、真实 Application Invocation、批量评测 Execution 和审批恢复调度不属于本阶段，仍由阶段 08、09 和 12 负责。

## 1. 已验收能力

- Application、不可变 Deployment、API Key 一次显示/轮换/撤销、Webhook、Schedule 和 Session 版本策略。
- Trigger Gateway 的 JWT/API Key 边界、Invocation 幂等、MySQL SSE 回放契约和 `RUNTIME_UNAVAILABLE` 默认 Adapter。
- Dataset Case 工作区、CSV/JSONL 原子导入导出、不可变 Dataset Version、四类确定性评分规则、不可变 Evaluation Profile Version、Evaluation Run 和空报告。
- Approval Claim、Release、Reassign、Approve、Reject、Cancel、Timeout 与 `blocked_runtime` 恢复边界。
- Notification 幂等投影、Header 收件箱、消息中心、已读和业务跳转。
- MySQL Execution Summary、Redis Trace Stream、Trace Writer、ClickHouse Trace 查询、脱敏和 MinIO Artifact 下载授权。
- Runtime Status 和 Dashboard 使用真实查询；无心跳显示 `unknown`，无运行数据展示真实空状态。

## 2. 快速门禁

执行：

```powershell
.\scripts\check.ps1
```

结果：通过。

- Rust fmt、Clippy `-D warnings` 和全部 Workspace Tests 通过。
- Platform API 22 项测试、Trigger Gateway 3 项测试、Trace Writer 1 项测试通过。
- 空数据库 Migration 幂等和 M1 Schema 向当前 Schema 追加 Migration 通过。
- Platform API 与 Trigger Gateway OpenAPI、两套 TypeScript 生成文件无漂移。
- Oxlint、11 个 Vitest 文件共 22 项测试、TypeScript 和 Vite 生产构建通过。
- Local、E2E、LightRAG 和 Mem0 Kustomize 清单均可渲染。

## 3. Kubernetes E2E

执行：

```powershell
.\scripts\e2e.ps1
```

结果：通过并自动删除临时 `agentx-e2e` Namespace。

| 阶段 | 测试 | 结果 |
|---|---|---|
| 控制面回归 | `m2.1-control-plane.spec.ts` | 1 passed |
| M3 控制面 | `m3-control-plane.spec.ts` | 1 passed |
| M3 可观测外围 | `m3-observability.spec.ts` | 1 passed |

测试通过真实页面按钮完成 Bootstrap、资源和 Workflow 基线、Application、Deployment、API Key、Webhook、Schedule、Session、Dataset、包含两条评分规则的 Evaluation Profile、Evaluation、审批、通知、Execution、Trace、Artifact 和 Runtime Status 操作。危险操作同时覆盖取消与确认，成员账号覆盖管理按钮隐藏和 403。评测模型和依赖交互的补充证据见 [M3.1 验收证据](m3.1-acceptance-evidence.md)。

第二阶段只通过 E2E Overlay 中的 `m3-fixture` Job 调用正式内部 Port 写入 Approval、Execution 和 Trace，不暴露生产伪造 HTTP API。证据脚本额外验证：

- Runtime 不可用时 `application_invocations` 为 0。
- Runtime 不可用时 `application_messages` 为 0。
- Evaluation 启动不可用时 `evaluation_case_results` 为 0。
- Trace UI 验收前后 `trace_delivery_outbox` 未交付数均为 0。
- Trace 页面能读取 `workflow.started`、`model.completed`，敏感属性已脱敏，并能下载有权访问的 Artifact。

测试源码和环境证据入口：

- [E2E 脚本](../../scripts/e2e.ps1)
- [M3 控制面 E2E](../../apps/e2e/tests/m3-control-plane.spec.ts)
- [M3 可观测性 E2E](../../apps/e2e/tests/m3-observability.spec.ts)
- `apps/e2e/playwright-report/`
- `apps/e2e/test-results/kubernetes/`

## 4. 完成边界

- APP-001～APP-011、EVA-001～EVA-010、OBS-001～OBS-011 和 M3-001～099 均完成。
- 阶段 05、06、07 和里程碑 M3 可以标记为 `done`。
- MVP 步骤 9～12 仍为 `planned`：Fixture 不代表真实批量评测、Application Execution 或审批恢复已经贯通。
- 下一阶段基线切换为 M4，即阶段 08 的确定性 Workflow Runtime Core。
