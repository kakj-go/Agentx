# Agentx 产品与架构文档

Agentx 是一个以 n8n 式 Workflow 编辑和运行模型为核心，面向企业 Agent 场景增强的平台。首期采用单公司部署和公司内多部门模型，数据实体继续保留 tenant_id 作为隔离边界。

平台的主链路是：

设计 Workflow → 手动调试 → 查看执行记录 → 测试集评测 → 发布版本 → 生成应用和 API → 生产运行 → 审批或恢复 → 分析 Trace、成本和错误

本文档集只关注 Workflow 业务上下游，不把项目扩展为通用 OA、通用低代码平台或通用企业基础设施平台。

## 文档目录

| 文档 | 内容 |
|---|---|
| [01-product-scope.md](01-product-scope.md) | 产品定位、边界、核心概念和业务闭环 |
| [02-system-architecture.md](02-system-architecture.md) | 系统分层、模块职责、部署单元和存储分工 |
| [03-workflow-engine.md](03-workflow-engine.md) | n8n 式数据模型、节点协议、表达式、调度和故障恢复 |
| [04-runtime-governance.md](04-runtime-governance.md) | Trace、Checkpoint、Agent 循环、CubeSandbox 和审批 |
| [05-platform-business.md](05-platform-business.md) | 多租户权限、模型与工具资源、应用会话和测试评测 |
| [06-data-model.md](06-data-model.md) | MySQL、ClickHouse、Redis、对象存储的数据设计 |
| [07-deployment.md](07-deployment.md) | Kubernetes 部署、水平扩容和运行可靠性 |
| [08-roadmap.md](08-roadmap.md) | 开发阶段、交付物、验收标准和 MVP 范围 |
| [09-codebase-architecture.md](09-codebase-architecture.md) | Monorepo、Rust 服务、公共 Crate 和依赖边界 |
| [10-frontend-architecture.md](10-frontend-architecture.md) | Tailwind UI 体系、企业工作台和 React Flow 画布 |
| [plan/README.md](plan/README.md) | 全量实施顺序、阶段任务、依赖、验收门禁和功能追踪 |
| [plan/m2-task-list.md](plan/m2-task-list.md) | M2 Workflow 控制面与资源中心的详细实施批次和任务清单 |
| [plan/m2.1-resource-redesign.md](plan/m2.1-resource-redesign.md) | M2.1 MCP、Skill Workspace、Kubernetes Addon 与全局 E2E 重构任务 |
| [plan/m2.1-acceptance-evidence.md](plan/m2.1-acceptance-evidence.md) | M2.1 当前验收状态和最终证据入口 |
| [plan/m3-task-list.md](plan/m3-task-list.md) | M3 应用入口、评测控制面与运行可观测外围任务 |
| [plan/m3-acceptance-evidence.md](plan/m3-acceptance-evidence.md) | M3 快速检查、Kubernetes E2E 和完成边界证据 |
| [plan/m3.1-evaluation-profile-and-prerequisites.md](plan/m3.1-evaluation-profile-and-prerequisites.md) | M3.1 评测方案合并与依赖型操作交互修正任务 |
| [plan/m3.1-acceptance-evidence.md](plan/m3.1-acceptance-evidence.md) | M3.1 契约、前端、Kubernetes E2E 和完成边界证据 |
| [plan/e2e-testing-standard.md](plan/e2e-testing-standard.md) | 临时 Kubernetes Playwright 端到端测试规范 |
| [plan/m2-acceptance-evidence.md](plan/m2-acceptance-evidence.md) | 已被 M2.1 取代的旧 M2 历史证据 |

## 阅读和实施顺序

1. 先阅读本页和 `01` 至 `10` 的产品、架构与工程边界。
2. 通过 [实施总计划](plan/README.md) 确认当前阶段、进入条件和全局完成定义。
3. 阅读对应阶段实施手册，按任务依赖推进并保存验收证据。
4. 完成任务时同步更新 [功能追踪矩阵](plan/99-feature-traceability.md)。

现有 `01` 至 `10` 文档回答“系统是什么以及为什么这样设计”，`plan/` 回答“按什么顺序实现、交付什么以及如何证明完成”。

## 核心设计结论

1. Workflow 是平台第一核心对象，其他功能都服务于其开发、运行、发布和评测。
2. 交互和运行语义尽量接近 n8n，但内部定义独立协议；n8n JSON 导入作为适配能力处理。
3. Workflow 草稿可变，Workflow Version 不可变，Deployment 负责将版本发布到环境。
4. 采用 Item 数据模型传递节点数据，保留 Item 的上下游来源关系。
5. MySQL 保存权威业务状态；Redis 只负责缓存、租约、限流和任务派发。
6. ClickHouse 首期只保存和查询 Workflow Trace，不要求接入 Prometheus 或 Grafana。
7. Trace、Checkpoint 和审计事件是三类不同数据，不能互相替代。
8. 历史节点重新执行创建 Fork Execution，不修改原执行记录。
9. 生产 Workflow 使用独立运行身份，不继承设计者个人权限。
10. 控制面可以模块化单体起步，但 Workflow Worker、Sandbox Manager 和 Trace Writer 必须能够独立扩容。
11. 前端使用 Tailwind CSS 设计令牌和统一组件层，复杂无障碍交互基于 Radix UI。
12. Workflow 画布使用 React Flow，画布状态通过转换层生成独立的 Workflow Definition。
13. 本地 Kustomize 同时启动应用、MySQL、Redis、ClickHouse 和 MinIO。

## 实施前详细设计

进入编码前还需要继续固化以下规范：

- Workflow Definition JSON Schema
- Workflow 编译后 IR
- Node SDK 与节点版本兼容规则
- 表达式语法和执行上下文
- Execution 状态机
- MySQL DDL 与索引
- 外部 API 和内部 gRPC 协议
- CubeSandbox Adapter 接口
- Trace Event Schema

这些规范分别在 [阶段 01](plan/01-contracts-and-foundation.md)、[阶段 08](plan/08-workflow-runtime-core.md)、[阶段 09](plan/09-checkpoint-wait-recovery.md) 和 [阶段 10](plan/10-agent-cubesandbox.md) 中完成并通过阶段门禁，不再作为无归属的开放事项保留。
