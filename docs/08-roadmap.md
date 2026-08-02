# 实施路线与验收标准

本文只保留产品级里程碑、实施顺序和 MVP 验收闭环。详细任务、依赖、接口、测试和完成定义见 [全量实施总计划](plan/README.md)。

## 1. 当前基线

M1 基础管理闭环已经完成：

- Rust Cargo Workspace、独立服务和公共 Crate
- React、TypeScript、Tailwind CSS 企业工作台
- 单公司 Bootstrap、JWT、Refresh Token 和首次修改密码流程
- 部门树、用户、角色、数据范围、审计和跨租户 Repository 防护
- SQLx Migration、OpenAPI、生成 TypeScript Client 和同源 `/api/v1`
- Docker 镜像、本地 Kubernetes、Migration Job、MySQL、Redis、ClickHouse 和 MinIO
- `/organization`、`/roles` 和 Header 用户状态使用真实 API

Workflow 和资源中心等后续业务页面仍使用 Mock，运行引擎尚未实现。下一里程碑是 M2，按 [M2 实施任务清单](plan/m2-task-list.md)建设 Workflow 控制面、资源版本和授权；与运行相关的功能仍不得伪造成功结果。

## 2. 总体实施顺序

平台采用外围控制面优先、完整运行引擎和 Studio 后置的混合路径：

1. 冻结最小 Workflow、Version、Execution 和资源引用契约。
2. 完成数据库、API、Bootstrap、JWT、租户和 RBAC。
3. 完成 Workflow 控制面和资源中心。
4. 完成 Application、Session、Dataset、Evaluation、Approval、Notification 和 Trace 外围能力。
5. 使用 JSON Fixture 实现确定性 Workflow 运行内核。
6. 完成 Checkpoint、Fork、Wait 和审批恢复。
7. 完成 Agent、Tool Loop、资源运行和 CubeSandbox。
8. 最后完善 n8n 式 Workflow Studio。
9. 打通应用、评测、审批、通知、配额和全链路发布验收。

外围接口在运行引擎可用前必须返回明确的 `RUNTIME_UNAVAILABLE`，不能通过伪 Execution、伪审批恢复或伪报告宣告完成。

## 3. 里程碑

| 里程碑 | 交付结果 | 详细计划 |
|---|---|---|
| M1 基础管理（done） | Bootstrap、JWT、单公司、部门、用户、角色和数据范围可用 | [阶段 01](plan/01-contracts-and-foundation.md)、[阶段 02](plan/02-bootstrap-auth-iam.md)；[验收证据](plan/m1-acceptance-evidence.md) |
| M2 控制面 | Workflow Draft/Version/Deployment 和 Model、Tool、Skill、RAG、Memory、Credential 可用 | [阶段 03](plan/03-workflow-control-plane.md)、[阶段 04](plan/04-resource-center.md)；[详细任务清单](plan/m2-task-list.md) |
| M3 外围闭环 | Application、Session、Dataset、Evaluation、Approval、Notification、Execution/Trace 查询可用 | [阶段 05](plan/05-applications-sessions-gateway.md) 至 [阶段 07](plan/07-approvals-notifications-trace.md) |
| M4 可靠运行 | 非 AI JSON Workflow、Checkpoint、Fork、Wait 和审批恢复可用 | [阶段 08](plan/08-workflow-runtime-core.md)、[阶段 09](plan/09-checkpoint-wait-recovery.md) |
| M5 Agent 运行 | Agent、Tool、Skill、RAG、Memory、成本、循环限制和 CubeSandbox 可用 | [阶段 10](plan/10-agent-cubesandbox.md) |
| M6 Studio | 拖拽、配置、表达式、部分执行、Pin Data、Trace、版本和发布可用 | [阶段 11](plan/11-workflow-studio.md) |
| M7 首期发布 | 应用、会话、审批、评测、Checkpoint、Trace、配额和通知全部贯通 | [阶段 12](plan/12-integration-hardening-release.md) |

## 4. MVP 验收闭环

MVP 完成时，用户必须能在全新部署中连续完成：

1. 初始化企业和 Admin，并使用 JWT 登录。
2. 创建部门、用户、角色和数据范围。
3. 接入 Credential、模型、Tool、Skill、LightRAG 和 Mem0。
4. 将所有直接和间接资源依赖授权给 Workflow。
5. 拖拽创建包含 Agent、Tool、Code 和审批的 Workflow。
6. 手动运行并查看节点输入输出、Attempt 和 Trace。
7. 查看模型、Tool、Agent 循环、Token、成本和错误。
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
