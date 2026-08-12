# 实施路线与验收标准

本文只保留产品级里程碑、实施顺序和 MVP 验收闭环。详细任务、依赖、接口、测试和完成定义见 [全量实施总计划](plan/README.md)。

## 1. 当前基线

M1 基础管理闭环、M2.1 资源中心重构、M3 外围控制面、M4 可靠运行、M5 Agent 功能和 M6 Workflow Studio 及当前 Kubernetes 部署基线已经完成；gVisor/Kata 等生产强隔离能力列为 M7 强化：

- Rust Cargo Workspace、独立服务和公共 Crate
- React、TypeScript、Tailwind CSS 企业工作台
- 单公司 Bootstrap、JWT、Refresh Token 和首次修改密码流程
- 部门树、用户、角色、数据范围、审计和跨租户 Repository 防护
- SQLx Migration、OpenAPI、生成 TypeScript Client 和同源 `/api/v1`
- Docker 镜像、本地 Kubernetes、Migration Job、MySQL、Redis、ClickHouse 和 MinIO
- `/organization`、`/roles` 和 Header 用户状态使用真实 API
- Workflow Draft/Version/Deployment 保留，资源模型改为 Credential、Model、MCP Server/Tool、在线 Skill Workspace、LightRAG 和 Mem0
- M2F-001～005、M3-001～099 与 M3F-001～005 已通过临时 Kubernetes Playwright 门禁
- Application、Gateway、Dataset、Evaluation、Approval、Notification、Execution/Trace 查询与 Runtime Status 已接入真实 API
- Definition 2.0、Node Protocol、Coordinator/Worker、Checkpoint/Fork、Wait/Approval 和 Execution/Recovery Workbench 已通过可靠性门禁
- M5 Runtime Port、资源 Adapter、Agent Ledger/循环、Rust OpenSandbox Adapter、Sandbox Manager 和 Runtime Workbench 已实现，Agent+MCP、Skill/RAG/Memory、全部 Command Runner、故障恢复和真实 OpenSandbox 链路已通过临时 Kubernetes E2E
- M6 Definition 4.0、Manifest 驱动画布/表单、Draft Revision 调试、Pin/Mock、Trace、Version/Deployment 和 Agent/Code/Approval 全 UI 链路已通过临时 Kubernetes E2E

完成证据见 [M2.1 验收证据](plan/m2.1-acceptance-evidence.md)、[M3 验收证据](plan/m3-acceptance-evidence.md)、[M3.1 验收证据](plan/m3.1-acceptance-evidence.md)、[M4 验收证据](plan/m4-acceptance-evidence.md)、[M5 验收证据](plan/m5-acceptance-evidence.md) 和 [M6 验收证据](plan/m6-acceptance-evidence.md)。当前 Docker Desktop Kubernetes+runc 是 M5/M6 的验收基线；生产 RuntimeClass、Vault、供应链和跨租户强隔离属于 M7 生产强化，不作为当前 M5/M6 阶段阻塞项。

## 2. 总体实施顺序

平台采用外围控制面优先、完整运行引擎后接 Studio、最后统一入口和生产发布的路径。M6/M7 的架构分工是：M6 负责可编辑、可调试、可发布的 Studio 与 Draft Debug Snapshot；M7 负责把不可变 Version 接入所有生产入口并完成安全/可靠性门禁。

1. 冻结最小 Workflow、Version、Execution 和资源引用契约。
2. 完成数据库、API、Bootstrap、JWT、租户和 RBAC。
3. 完成 Workflow 控制面和资源中心。
4. 完成 Application、Session、Dataset、Evaluation、Approval、Notification 和 Trace 外围能力。
5. 使用 JSON Fixture 实现确定性 Workflow 运行内核。
6. 完成 Checkpoint、Fork、Wait 和审批恢复。
7. 完成 Agent、MCP Tool Loop、资源运行和 OpenSandbox。
8. 完成 Agentx 原生、n8n-like 交互的 Workflow Studio，并冻结 Definition/Editor/Overlay 三层边界。
9. 用同一个 ExecutionRuntime 接通应用、Trigger、评测、审批、通知、配额和生产发布。

尚未接入 M5 Runner 或 M7 真实 Execution 的外围入口必须返回明确的 `RUNTIME_UNAVAILABLE`，不能通过伪 Execution、伪资源运行或伪报告宣告完成。

## 3. 里程碑

| 里程碑 | 交付结果 | 详细计划 |
|---|---|---|
| M1 基础管理（done） | Bootstrap、JWT、单公司、部门、用户、角色和数据范围可用 | [阶段 01](plan/01-contracts-and-foundation.md)、[阶段 02](plan/02-bootstrap-auth-iam.md)；[验收证据](plan/m1-acceptance-evidence.md) |
| M2 控制面（done） | Workflow Draft/Version/Deployment 和 Model、MCP、Skill Workspace、RAG、Memory、Credential、统一授权可用 | [阶段 03](plan/03-workflow-control-plane.md)、[阶段 04](plan/04-resource-center.md)；[M2.1 任务](plan/m2.1-resource-redesign.md)；[验收证据](plan/m2.1-acceptance-evidence.md) |
| M3 外围闭环（done） | Application、Session、Dataset、Evaluation、Approval、Notification、Execution/Trace 查询可用 | [阶段 05](plan/05-applications-sessions-gateway.md) 至 [阶段 07](plan/07-approvals-notifications-trace.md)；[M3 任务](plan/m3-task-list.md)与[验收证据](plan/m3-acceptance-evidence.md)；[M3.1 修正](plan/m3.1-evaluation-profile-and-prerequisites.md)与[验收证据](plan/m3.1-acceptance-evidence.md) |
| M4 可靠运行（done） | 非 AI JSON Workflow 按 n8n 行为语义运行，Checkpoint、Fork、Wait 和审批恢复可用 | [M4 任务](plan/m4-task-list.md)、[阶段 08](plan/08-workflow-runtime-core.md)、[阶段 09](plan/09-checkpoint-wait-recovery.md)；[验收证据](plan/m4-acceptance-evidence.md) |
| M5 Agent 运行（done） | AGT-001～013、Go Oracle、资源/Agent/Sandbox 故障矩阵和当前 Docker+runc Runtime E2E 已完成；AGT-010 生产强化暂不重试并转入 M7 | [阶段 10](plan/10-agent-opensandbox.md)、[任务清单](plan/m5-task-list.md)、[验收证据](plan/m5-acceptance-evidence.md)、[OpenSandbox 可行性](plan/opensandbox-feasibility.md) |
| M6 Studio（done） | Definition 4.0、Manifest 驱动画布、Draft Revision 调试快照、Pin/Mock、Trace、版本发布和 Agent/Code/Approval 全 UI E2E | [阶段 11](plan/11-workflow-studio.md)；[验收证据](plan/m6-acceptance-evidence.md) |
| M7 首期发布 | 复用 M6 Runtime 接通 Application/Trigger/Evaluation/Approval/SSE，完成配额、保留、生产 RuntimeClass、Vault、签名镜像、跨租户安全、容量和发布证据 | [阶段 12](plan/12-integration-hardening-release.md) |

## 4. MVP 验收闭环

MVP 完成时，用户必须能在全新部署中连续完成：

1. 初始化企业和 Admin，并使用 JWT 登录。
2. 创建部门、用户、角色和数据范围。
3. 接入 Credential、模型、MCP Server/Tool、Skill Workspace、LightRAG 和 Mem0。
4. 将所有直接和间接资源依赖授权给 Workflow。
5. 拖拽创建包含 Agent、MCP Tool、Code 和审批的 Workflow。
6. 手动运行并查看节点输入输出、Attempt 和 Trace。
7. 查看模型、MCP Tool、Agent 循环、Token、成本和错误。
8. 从历史 Checkpoint 创建 Fork Execution。
9. 使用 Dataset Version 批量评测 Workflow Version。
10. 将 Workflow Version 发布为 Application Deployment。
11. 通过 Playground 和 API 创建 Session、Message 并接收 SSE。
12. 在待办页面审批并恢复原 Workflow Execution。

每一步的唯一任务归属和验收证据见 [功能追踪矩阵](plan/99-feature-traceability.md)。

## 5. 首期边界

首期暂缓：

- 大量第三方连接器和插件市场
- n8n JSON 导入与全量节点兼容
- 通用 BPMN、通用 OA 和通用页面搭建
- Prometheus、Grafana 和通用监控平台
- 完整商业计费平台
- 跨区域多活
- 首期 OIDC 登录实现；只预留 Identity Provider Adapter

## 6. 阶段完成规则

- 不使用百分比判断完成。
- 阶段进入前必须满足上一阶段稳定接口和进入条件。
- 领域、Migration、API、前端和测试交付物必须同时完成。
- 阶段验收门禁全部通过后才能标记为 `done`。
- 任何架构边界变更必须先更新设计文档、决策记录和追踪矩阵。
- 首期发布前 [功能追踪矩阵](plan/99-feature-traceability.md) 不得存在 `planned`、`in_progress` 或 `blocked`。
