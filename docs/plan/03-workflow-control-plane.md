# 阶段 03：Workflow 控制面

## 1. 目标与用户价值

在完整运行引擎和画布之前建立 Workflow 的业务容器、草稿、不可变版本、部署和授权身份，让所有外围模块能够引用稳定对象。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：[阶段 02](02-bootstrap-auth-iam.md) 的租户、权限和 Workflow Service Identity 基础完成。
- 本阶段不依赖 Scheduler，但必须为运行时提供不可变 Version Snapshot。
- M2 实际开发顺序、资源阶段交叉依赖和子任务见 [M2 实施任务清单](m2-task-list.md)。

## 3. 范围和不做内容

实现 Workflow 元数据、Draft Revision、Version、Deployment、Environment、发布校验和资源授权外壳。不实现节点执行、表达式解释、复杂图编译、画布编辑或 n8n JSON 导入。

## 4. 领域对象、状态和不变量

- Workflow 状态为 `active` 或 `archived`；归档不会删除 Version 和 Execution 引用。
- 每个 Workflow 只有一个当前 Draft，可产生连续 Draft Revision。
- Draft 更新使用 revision number 和乐观锁，保存失败不得覆盖新内容。
- Workflow Version 创建后完全不可修改，并保存 definition_json、content_hash 和资源快照。
- Deployment 属于明确 Environment，状态为 `draft`、`active`、`superseded` 或 `rolled_back`。
- 发布和回滚只切换 Deployment 指针，不修改历史 Version。
- Workflow Service Identity 与 Workflow 一对一，生产执行只使用该身份。

## 5. 数据和 Migration

主要表：

- workflows、workflow_members
- workflow_drafts、workflow_draft_revisions
- workflow_versions、workflow_version_resources
- environments、workflow_deployments、deployment_history
- workflow_service_identities、workflow_resource_grants

Definition 以 JSON 保存并包含 schemaVersion。本阶段允许最小 Definition，但保存 Version 时必须做结构、引用和 Content Hash 校验。

## 6. REST API、Port 和事件

- `/api/v1/workflows`
- `/api/v1/workflows/{id}/draft`
- `/api/v1/workflows/{id}/revisions`
- `/api/v1/workflows/{id}/versions`
- `/api/v1/workflows/{id}/deployments`
- `/api/v1/workflows/{id}/members`
- `/api/v1/workflows/{id}/resource-grants`

Application Port：`WorkflowRepository`、`DraftRepository`、`VersionRepository`、`DeploymentRepository`、`PublishValidator` 和 `WorkflowAuthorization`。

业务事件：`WorkflowCreated`、`DraftRevised`、`WorkflowVersionCreated`、`WorkflowPublished`、`WorkflowRolledBack`、`WorkflowArchived`。

## 7. 后端和前端改动

- Platform API 增加 workflow-catalog 模块，不进入 Coordinator 或 Worker。
- Domain 层增加 Workflow、Draft、Version、Deployment 和 Environment 值对象。
- API DTO 保持 Definition 与数据库实体分离。
- `/workflows` 接入真实列表、新建和归档。
- 增加 Workflow 详情、成员、版本、部署、权限和资源授权页面。
- `/workflows/:workflowId/editor` 本阶段只读取/保存最小 Draft，不宣称支持真实运行。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| WCP-001 | planned | IAM-005–006 | Workflow、Member 和 Service Identity Schema | 创建 Workflow 同事务创建运行身份 |
| WCP-002 | planned | WCP-001 | Draft、Revision 和并发保存用例 | 旧 revision 更新返回冲突且保留双方内容 |
| WCP-003 | planned | WCP-002 | 最小 Workflow Definition Schema 和 Canonical Hash | 字段顺序不影响 Hash，非法 Schema 不能生成 Version |
| WCP-004 | planned | WCP-003 | 不可变 Workflow Version 和资源快照 | Version API 不提供更新或删除历史内容能力 |
| WCP-005 | planned | WCP-004 | Environment、Deployment、发布和回滚 | 同环境只有一个 Active Deployment |
| WCP-006 | planned | IAM-005、WCP-001 | Workflow 成员、数据范围和 Resource Grant 外壳 | 非成员且无数据范围用户无法查看 Workflow |
| WCP-007 | planned | WCP-004–006 | Publish Validator | 缺失资源、无授权或 Schema 错误阻止发布 |
| WCP-008 | planned | WCP-001–007 | Workflow REST API、OpenAPI 和审计事件 | 契约测试覆盖创建、并发、版本、发布和回滚 |
| WCP-009 | planned | WCP-008、FND-011 | Workflow 列表、详情、版本和部署页面 | 页面使用真实 API 并显示冲突、空状态和发布失败原因 |
| WCP-010 | planned | WCP-006、WCP-009 | 成员和资源授权界面 | 授权变更受权限控制并产生审计记录 |

## 9. 失败、安全和幂等边界

- 创建 Version 使用 Draft revision 和 Content Hash 幂等，重复请求不产生不同内容的同号版本。
- 发布在事务内校验目标 Version、Environment 和当前 Deployment。
- Resource Grant 撤销不修改历史快照，但后续新执行仍需运行时二次校验。
- 归档 Workflow 阻止新发布和新调用，不删除会话、Execution 或报告。
- Draft 内容不允许嵌入 Credential 明文。

## 10. 测试

- Canonical JSON、Hash、版本不可变和状态迁移单元测试。
- 并发 Draft 保存、并发发布、回滚和归档集成测试。
- Workflow 成员和数据范围权限矩阵测试。
- OpenAPI 契约与前端 Version/Deployment 页面测试。
- 审计记录与历史 Version 可复现性测试。

## 11. 验收门禁

- Draft 可编辑且 Revision 可追溯，Version 不可修改。
- Deployment 只指向明确 Version，同环境激活状态唯一。
- 外围模块可以稳定引用 Workflow 和 Version ID。
- 发布前资源存在性和授权检查可用，不依赖运行引擎。
- Workflow 页面不再使用 Mock 数据。

## 12. 对后续阶段的稳定输出

- Workflow、Draft、Version、Environment 和 Deployment Repository/API。
- Workflow Service Identity 和 Resource Grant 绑定点。
- 不可变 Definition 与 Resource Snapshot。
- 发布、回滚和归档事件。
