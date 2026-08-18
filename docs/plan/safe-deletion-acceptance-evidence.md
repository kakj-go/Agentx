# 业务域国际化与安全删除验收证据

## 1. 完成边界

- 前端翻译资源已由里程碑命名迁移为业务域模块，`common` 只承载跨业务同义文案；中英文键结构、禁止里程碑 namespace、业务域词条所有权、稳定错误码、Model/Skill Alias 隔离均有静态回归测试。
- Platform API 提供统一分页删除影响接口，16 类实体保留各自 REST DELETE。DELETE 使用目标版本、行锁、同一引用检查、聚合清理和审计事务。
- Draft、Workflow Version、Skill Dependency、Resource Grant、直接外键和运行历史纳入引用来源。Draft 使用结构化 `workflow_draft_resources` 投影，不对 JSON 做模糊匹配。
- 列表统一使用带“删除”可见文字的共享删除按钮和 Dialog，不使用仅图标删除入口；权限、系统实体、引用分组、分页、409 最新影响替换、成功刷新和危险操作状态均有前端测试。
- User、API Key、不可变 Version/Deployment、Execution、Approval、Notification、Evaluation Run/Report、Session 和 Message 不增加物理删除。

## 2. 自动化证据

| 层级 | 命令或测试 | 结果 |
|---|---|---|
| Rust workspace | `cargo check --workspace` | 通过 |
| Platform API 完整套件 | `cargo test -p platform-api -- --test-threads=1` | 50/50 通过 |
| 后端真实 MySQL | `cargo test -p platform-api deletion::tests -- --nocapture` | 4/4 通过 |
| Web 单元/组件 | `pnpm --filter @agentx/web test` | 全量通过（含动态 i18n 词汇和安全删除组件） |
| Web 生产构建 | `pnpm --filter @agentx/web build` | 通过 |
| Kubernetes UI E2E | `./scripts/e2e.ps1 -OnlySuite deletion` | 2026-08-11 重新构建当前工作树镜像：M2.1 1/1、Safe Deletion 2/2 通过，0 skipped；临时命名空间已清理 |

真实 MySQL 测试覆盖 Draft 引用、跨租户隔离、预检后新增引用的 409 复检、结构化 409 details、目标版本冲突、Credential 聚合清理、审计事件、历史 MCP Version 引用、Department Connection 引用、Application Deployment 对 Environment 的阻止及根部门保护；表驱动测试覆盖 16 类实体、独立删除权限和不可物理删除的历史实体集合。

前端组件测试覆盖无权限隐藏、系统实体禁用、无引用预检与确认、成功刷新、引用分组与分页，以及 DELETE 竞态 409 后使用最新引用更新 Dialog 并继续分页加载。Platform API 权限列表回归断言当前 70 个权限，包含新增的 13 个 `*:delete` 权限。

迁移器增加了 `migrations/mysql` 目录变更监听，确保新增迁移会重新编译进 `sqlx::migrate!`；空库连续执行两次迁移的回归已包含在 50 项 Platform API 完整套件中。

## 3. Kubernetes 场景

`apps/e2e/tests/safe-deletion.spec.ts` 在 M2.1 真实 UI 数据基础上验证 Credential、Model、MCP Tool 间接引用和 Skill Grant 的删除阻止；创建无引用 Credential 验证取消与确认；创建无引用 Environment 并删除；检查内置 Environment、内置 Role 和根 Department 的禁用删除入口。第二个场景自包含创建 Application Deployment、Gateway Session/Invocation、Dataset Case/Version、Evaluation Profile/Run，随后通过可见列表打开 Application、Dataset、Workflow 删除 Dialog，并用预检返回的 Session/Invocation、Evaluation Run（不可变）和 Application 引用做 UI 阻止回归，不依赖 M3/M4/M7 测试顺序。

E2E 使用 `agentx-e2e` 临时 namespace，构建并部署当前代码镜像。脚本结束时卸载服务、删除 namespace，并恢复测试前 `agentx` namespace 的工作负载副本数。

运行 `20260810T183425620Z` 的 JUnit 记录显示 M2.1 为 1 个测试、Safe Deletion 为 2 个测试，0 skipped、0 失败。验收后确认 `agentx-e2e` namespace 已删除，`agentx` 下全部 Deployment 和 StatefulSet 均恢复为期望副本 `1` 且 Ready `1`。

## 4. 契约定位

- OpenAPI：`openapi/platform-api.json`
- Migration：`migrations/mysql/0023_safe_entity_deletion.sql`
- 后端实现：`services/platform-api/src/deletion.rs`
- 前端共享交互：`apps/web/src/shared/components/entity-delete-button.tsx`、`entity-delete-dialog.tsx`
- i18n 资源：`apps/web/src/app/i18n/locales/zh-CN`、`en-US`
- E2E：`apps/e2e/tests/safe-deletion.spec.ts`
