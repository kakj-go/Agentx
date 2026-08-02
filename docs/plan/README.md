# Agentx 全量实施总计划

本目录将产品与架构文档转化为可执行任务。设计依据仍以 [产品与架构文档索引](../README.md) 为准；本目录负责实现顺序、依赖、交付物、测试和验收门禁。

## 1. 当前基线

- M1 基础管理闭环已经完成：单公司 Bootstrap、JWT、部门、用户、角色、数据范围、OpenAPI 和 Kubernetes Migration Job 可用。
- `/organization`、`/roles` 和 Header 用户状态已接入真实 API；Workflow 等后续业务页面继续使用 Mock，并受真实认证路由保护。
- Workflow 控制面、资源中心、运行引擎、Trace 写入和 CubeSandbox Adapter 尚未实现，下一阶段从阶段 03 开始。

因此后续不能以“逐页替换 Mock”的方式推进。所有业务先建立稳定契约和权威数据，再接入页面；与运行相关的外围接口在引擎可用前必须返回明确的 `RUNTIME_UNAVAILABLE`，不得伪造成功记录。

## 2. 实施顺序

    最小 Workflow 契约
            ↓
    基础设施、认证和 IAM
            ↓
    Workflow 控制面与资源中心
            ↓
    Application、Dataset、Approval、Notification、Trace 外围能力
            ↓
    Workflow 确定性运行内核
            ↓
    Checkpoint、Wait 和审批恢复
            ↓
    Agent、资源运行和 CubeSandbox
            ↓
    n8n 式 Workflow Studio
            ↓
    全链路集成、加固和发布

完整引擎和 Studio 后置，但 Workflow、Draft、Version、Deployment、Execution、Resource Reference 和运行命令必须前置冻结。

## 3. 阶段状态

状态只允许使用 `planned`、`in_progress`、`blocked` 和 `done`。阶段必须通过全部验收门禁后才能标记为 `done`。

| 阶段 | 文档 | 状态 | 核心退出条件 |
|---|---|---|---|
| 00 | [基线与决策](00-baseline-and-decisions.md) | done | 首期边界和全局决策无阻塞项 |
| 01 | [契约和公共基础](01-contracts-and-foundation.md) | done | Migration、统一 API、Tenant Context 和依赖 Readiness 可用 |
| 02 | [初始化、认证和 IAM](02-bootstrap-auth-iam.md) | done | Bootstrap、JWT、单公司隔离和 RBAC 可用 |
| 03 | [Workflow 控制面](03-workflow-control-plane.md) | planned | Draft、Version、Deployment 和资源快照边界稳定 |
| 04 | [资源中心](04-resource-center.md) | planned | Credential 与全部资源授权可用 |
| 05 | [应用、会话和调用入口](05-applications-sessions-gateway.md) | planned | Application、API Key、Session、Message 和 Gateway 契约可用 |
| 06 | [测试集和评测](06-datasets-evaluations.md) | planned | 不可变 Dataset Version 和 Evaluation 控制面可用 |
| 07 | [审批、消息和 Trace](07-approvals-notifications-trace.md) | planned | 外围任务、通知、Trace 写入与查询可用 |
| 08 | [Workflow 运行内核](08-workflow-runtime-core.md) | planned | 非 AI JSON Fixture 可分布式可靠执行 |
| 09 | [恢复、等待和审批运行](09-checkpoint-wait-recovery.md) | planned | Checkpoint、Fork、Wait 和审批恢复可用 |
| 10 | [Agent 与 CubeSandbox](10-agent-cubesandbox.md) | planned | Agent 在授权和沙箱约束内执行 |
| 11 | [Workflow Studio](11-workflow-studio.md) | planned | 可拖拽、调试、版本化和发布真实 Workflow |
| 12 | [集成、加固和发布](12-integration-hardening-release.md) | planned | MVP 十二步闭环和故障场景全部通过 |

功能覆盖和验收证据统一维护在 [功能追踪矩阵](99-feature-traceability.md)。

M2 的阶段 03、04 采用交错实施顺序，详细批次、子任务、Migration、权限种子和验收流程见 [M2 实施任务清单](m2-task-list.md)。

## 4. 里程碑

| 里程碑 | 结果 | 对应阶段 |
|---|---|---|
| M1 | Bootstrap、JWT、单公司、部门、用户和 RBAC 可用（done） | 01–02 |
| M2 | Workflow 控制面和全部资源管理可用 | 03–04；[详细任务清单](m2-task-list.md) |
| M3 | Application、Session、Dataset、Approval、Notification、Trace 外围能力可用 | 05–07 |
| M4 | JSON Fixture 定义的非 AI Workflow 能可靠执行 | 08–09 |
| M5 | Agent、Tool、Skill、RAG、Memory 和 CubeSandbox 能执行 | 10 |
| M6 | n8n 式 Workflow Studio 可拖拽、调试和发布 | 11 |
| M7 | 应用调用、审批、Checkpoint、评测、Trace 和通知全链路贯通 | 12 |

## 5. 任务格式

每个阶段的任务表必须包含：

- 唯一任务编号。
- 当前状态。
- 明确依赖。
- 可交付的代码、Schema、页面或文档。
- 可自动或人工验证的验收条件。

任务不得使用百分比。只有实际交付物和验收证据齐备后才标记为 `done`；外部依赖或决策阻塞时标记为 `blocked` 并记录原因。

## 6. 全局完成定义

每项业务能力必须同时满足：

1. 领域不变量和权限边界有自动化测试。
2. MySQL Migration 支持空库初始化和滚动升级。
3. 外部 API 进入 OpenAPI，前端使用生成类型或客户端。
4. 所有查询携带 tenant_id 约束，资源详情再次校验数据范围。
5. 写操作定义幂等、并发冲突和审计行为。
6. 页面具备加载、错误、空状态、权限不足和中英文文案。
7. 浅色、深色和键盘焦点符合统一组件规范。
8. 单元、集成、契约和阶段端到端测试通过。
9. Rust、Oxlint、TypeScript、Vite 构建和 Kubernetes 清单检查通过。
10. 实现与架构边界一致；发生变更时先更新对应设计文档。

## 7. 计划维护规则

- 开始阶段时，将阶段和第一项实际任务标记为 `in_progress`。
- 新任务必须放入唯一阶段并同步追踪矩阵，避免重复归属。
- 阶段内可以调整实现顺序，但不得绕过进入条件和验收门禁。
- 接口发生破坏性变化时，更新决策记录、依赖阶段和迁移策略。
- 验收证据记录测试命令、测试文件或可复现的人工检查步骤。
- 不通过 Mock 运行结果、虚假审批恢复或虚假评测报告宣告能力完成。

## 8. 架构文档映射

| 设计依据 | 主要实施阶段 |
|---|---|
| [产品定位与范围](../01-product-scope.md) | 全部阶段和追踪矩阵 |
| [系统架构](../02-system-architecture.md) | 01、05、07、08、10、12 |
| [Workflow 运行引擎](../03-workflow-engine.md) | 03、08、09、11 |
| [运行治理](../04-runtime-governance.md) | 07、09、10、12 |
| [平台业务模块](../05-platform-business.md) | 02–07、10、12 |
| [数据模型](../06-data-model.md) | 01–10 |
| [Kubernetes 部署](../07-deployment.md) | 01、08、10、12 |
| [代码仓库架构](../09-codebase-architecture.md) | 全部阶段 |
| [前端架构](../10-frontend-architecture.md) | 02–07、11、12 |
