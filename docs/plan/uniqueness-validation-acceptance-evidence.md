# 用户输入唯一性校验验收证据

> 状态：实现完成（2026-08-12）。现有自动化门禁与 Kubernetes UI E2E 已通过；补充的真实业务 API 并发矩阵、逐路径 Artifact 故障注入，以及 MCP/Skill/Sandbox 唯一性专项 UI 场景由项目负责人后续人工验收，不作为本轮实现完成的阻塞项。

## 1. 完成边界

本次修复保留全部既有 MySQL 唯一索引，不新增数据库迁移、数据模型或实时查重端点。用户输入型唯一字段统一执行“规范化 → 提交前查询 → 唯一索引并发兜底 → 字段级 409”；未登记的唯一索引返回 `INTERNAL_ERROR`，日志只记录索引名、数据库错误码和 requestId。

| 对象 | MySQL 唯一索引 | 错误码 | 字段 |
|---|---|---|---|
| 模型名称 | `uq_model_alias` | `MODEL_NAME_EXISTS` | `alias` |
| 同级部门名称 | `uq_departments_sibling_name` | `DEPARTMENT_NAME_EXISTS` | `name` |
| 用户名 | `uq_users_tenant_username` | `USERNAME_EXISTS` | `username` |
| 角色编码 | `uq_roles_tenant_code` | `ROLE_CODE_EXISTS` | `code` |
| 应用 Slug | `uq_application_slug` | `APPLICATION_SLUG_EXISTS` | `slug` |
| Workflow 环境编码 | `uq_workflow_environment_code` | `ENVIRONMENT_CODE_EXISTS` | `code` |
| MCP Server 名称 | `uq_mcp_server_name` | `MCP_SERVER_NAME_EXISTS` | `name` |
| Skill 名称 | `uq_skill_name` | `SKILL_NAME_EXISTS` | `name` |
| Skill Alias | `uq_skill_alias` | `SKILL_ALIAS_EXISTS` | `alias` |
| Skill 工作区路径 | `uq_skill_workspace_path` | `SKILL_PATH_EXISTS` | `name`；上传/导入为 `file` |
| Sandbox Profile 名称 | `uq_sandbox_profile_name` | `SANDBOX_PROFILE_NAME_EXISTS` | `name` |
| Dataset Case Key | `uq_dataset_case_key` | `DATASET_CASE_KEY_EXISTS` | `caseKey`；导入为 `file` |
| Knowledge 外部资源 ID | `uq_rag_resource_external` | `KNOWLEDGE_EXTERNAL_RESOURCE_ID_EXISTS` | `externalResourceId` |
| Memory 外部 Namespace | `uq_memory_namespace_external` | `MEMORY_EXTERNAL_NAMESPACE_EXISTS` | `externalNamespace` |

## 2. 更新边界

公开 API 当前允许修改的唯一字段是模型名称、部门同级名称/父部门、MCP 名称、Skill 名称、Skill Alias、Skill 工作区路径、Sandbox Profile 名称和 Dataset Case Key。集成测试覆盖“保持自身原值成功”和“改为其他实体值返回领域错误”。用户名、角色编码、Application Slug、环境编码、Knowledge 外部 ID 和 Memory 外部 Namespace 在当前公开更新 API 中不可变；本次没有为了测试新增越界更新接口。

## 3. 专项行为

- MCP 元数据更新且配置 Hash 未变时不创建版本；命中历史 Hash 时复用不可变版本；全新配置按历史最大版本号加一。
- Skill 相同内容重复发布返回已有版本和 `200 OK`，不重复写 Version、File 或 Dependency。
- Skill 创建、元数据联动、文件创建、上传、移动重写、Markdown 更新和 ZIP 导入的 Artifact 在事务失败时补偿对象、元数据和配额；补偿失败进入结构化日志并由 Retention 兜底。
- Dataset 导入先检查文件内重复，再批量检查数据库；错误 `details` 提供可确定的 `caseKey`、`line` 和 `firstLine`。
- 前端共享表单和 Organization/Roles 自定义表单均显示、关联、聚焦并按字段清除 `fieldErrors`。Knowledge/Memory 表单字段名与公开契约对齐。

## 4. 自动化证据

后端真实 MySQL 合约测试：

```text
cargo test -p platform-api uniqueness_contract_tests --no-fail-fast
6 passed, 0 failed
```

该组测试包含 14 个真实唯一索引的双并发写入、8 个可修改字段的自身原值/重复更新、未知索引、Dataset 单条/编辑/导入、Skill 幂等发布及 ZIP Artifact 补偿。最终后端全量门禁为：

```text
cargo test -p platform-api
62 passed, 0 failed
```

前端针对性组件与页面测试：

```text
pnpm --filter @agentx/web exec vitest run \
  src/shared/components/entity-form-dialog.test.tsx \
  src/features/uniqueness-field-errors.test.tsx \
  src/app/i18n/index.test.ts
27 passed, 0 failed
```

覆盖 Input、Select、Textarea、表单摘要、聚焦、ARIA、逐字段清除、Organization、Roles，以及 Model、Application、MCP、Skill、Sandbox、Dataset、Knowledge、Memory 和 Environment 页面。中英文稳定错误码和未知 4xx/5xx 回退由 `src/app/i18n/index.test.ts` 覆盖。

最终前端全量门禁为 45 个测试文件、181 项测试全部通过；`tsc --noEmit`、`oxlint` 和生产构建通过。Lint 仅保留与本修复无关的既有 Fast Refresh/Hook 警告。

OpenAPI 在 `components.x-uniqueness-error-examples` 发布全部 14 个稳定错误码示例，并额外发布 Dataset 导入行号示例；生成源位于 Platform API，提交的 JSON 和 TypeScript 类型由标准生成命令同步。

## 5. Kubernetes UI E2E

唯一性验收使用当前源码重新构建镜像后，由 `scripts/e2e.ps1 -OnlySuite uniqueness` 创建独立 `agentx-e2e` Namespace，通过 1440×900 Chromium 的可见表单操作验证受影响资源。

- Run ID：`20260812T063639290Z`
- 结果：1 passed，0 failed，浏览器用例耗时 33.5 秒
- JUnit：`apps/e2e/test-results/kubernetes/20260812T063639290Z/m2.1-control-plane/junit.xml`
- HTML Report：`apps/e2e/test-results/kubernetes/20260812T063639290Z/m2.1-control-plane/playwright-report/index.html`
- 覆盖：中文模型名称、部门、用户名、角色、环境、MCP、Skill、Sandbox、Dataset、Knowledge、Memory 字段错误，以及英文 Application Slug
- 清理：运行结束后 `kubectl get namespace agentx-e2e --ignore-not-found` 无输出，临时 Namespace 已删除
- 恢复：共享 `agentx` 的 11 个 Deployment 与 4 个 StatefulSet 均恢复为 `1/1 Ready`

## 6. Schema 结论

本修复没有新增唯一性相关 Migration；数据库约束继续作为并发最终保护，应用前检只改善反馈和避免昂贵副作用，不能替代索引。

## 7. 后续人工验收边界

本轮按项目负责人决定标记实现完成。以下增强验证由项目负责人后续执行，不作为当前状态的未完成项：

- 通过每个真实业务 Handler/API 发起双并发请求，补充验证 14 类失败方的 HTTP 状态、领域错误码和 `fieldErrors`。
- 对 Skill 创建、元数据联动、文件创建、上传、移动重写、Markdown 更新和 ZIP 导入逐路径注入事务失败，核对对象、Artifact 元数据和配额。
- 通过真实 UI 补充 MCP 名称/原样保存、Skill 名称/Alias/重复路径和 Sandbox Profile 名称专项场景。
