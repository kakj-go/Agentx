# 画布资源授权申请验收证据

状态：`done`。最终复核日期：2026-08-11。最终 Kubernetes Run ID：`20260811T091319636Z`。

本文证明 Workflow Studio 的资源选择、直接授权、设计期申请、跨部门会签、通知、脱敏和授权后运行已经形成独立于运行时 Approval 的完整闭环。

## 1. 实现边界

| 能力 | 实现位置 | 关键约束 | 状态 |
|---|---|---|---|
| 六态资源选项与操作感知分页 | Platform API、`use-resource-options.ts`、`ResourcePicker` | `authorized/grantable/requestable/pending/rejected/unavailable`；Query 按 Resource Type 与 Operation 隔离 | done |
| 直接授权 | Platform API、Workflow Studio | 完整依赖包原子授权、Idempotency Key、成功后不自动选择 | done |
| 设计期授权申请 | 独立 Request/Item/Review 表与 API | 开放申请去重，终态后可重新申请，不复用 `approval_tasks` | done |
| 跨部门会签 | Platform API、待审批中心 | 每部门一项 Review，全部通过才创建 Grant，任一拒绝整包拒绝 | done |
| 权限与脱敏 | IAM、资源可见性查询、申请详情 | Company 数据范围可见；审批人只看到本部门资源详情，跨部门依赖脱敏 | done |
| 通知与审计 | Notifications、Audit Events、申请详情 | 创建、审批、拒绝、取消、失效和完成留痕并通知相关用户 | done |
| 画布失效处理 | Workflow Studio、保存/版本/运行校验 | 撤权或不可用资源保留引用、标红并阻止保存、版本创建和运行 | done |

## 2. 自动化门禁

同一工作树已通过：

- Web：41 个测试文件、152 项测试，Production Build 和 TypeScript 构建通过；Lint 仅保留 4 个与本功能无关的既有 Fast Refresh/Hook Warning。
- Platform API：`cargo check -p platform-api` 通过，完整测试 53 项通过。
- E2E：Playwright TypeScript 编译和测试收集通过。
- 文件边界：`resource_access.rs`、`workflow-canvas.tsx`、`node-inspector.tsx` 和本 E2E Spec 均低于 2000 行。

## 3. Kubernetes E2E

最终命令：

```powershell
.\scripts\e2e.ps1 -OnlySuite resource-grants -SkipBuild
```

最终 Run `20260811T091319636Z` 在全新 `agentx-e2e` Namespace 中验证：

1. Company 数据范围的编辑者能看到跨部门 Model，但没有为 Workflow Service Identity 授权时不能选择。
2. Company Admin 直接授权完整 Model/Credential 包，确认后资源变为可选择但不自动写入画布。
3. 普通 Workflow 编辑者提交带 Source Node、Draft Revision 和说明的申请。
4. Model 与 Credential 分属两个部门，两个 Department Admin 分别收到通知并只处理自己的 Review。
5. Model 审批人能看到 Model、看不到 Credential 明细；Credential 审批人能看到 Credential、看不到 Model 明细。
6. 两个 Review 全部通过后原子创建完整 Grant 包；编辑者回到画布手动选择、保存 Draft、创建 Version 并运行到 `completed`。
7. 另一申请被拒绝后 Picker 显示“重新申请”，且拒绝路径不创建 Grant。

证据：

- [资源授权 JUnit](../../apps/e2e/test-results/kubernetes/20260811T091319636Z/resource-grant-requests/junit.xml)：`tests=1, failures=0, skipped=0, errors=0`。
- [资源授权 Playwright 报告](../../apps/e2e/test-results/kubernetes/20260811T091319636Z/resource-grant-requests/playwright-report/index.html)。
- [前置控制面 JUnit](../../apps/e2e/test-results/kubernetes/20260811T091319636Z/m2.1-control-plane/junit.xml)：`tests=1, failures=0, skipped=0, errors=0`。
- [测试前副本快照](../../apps/e2e/test-results/kubernetes/20260811T091319636Z/agentx-original-replicas.json)。

测试结束后 `agentx-e2e` Namespace 已删除；`agentx` 的 11 个 Deployment 和 4 个 StatefulSet 均恢复为 `1/1 Ready`。

## 4. 开发环境升级

最终验收后执行：

```powershell
.\scripts\deploy.ps1 -Action Upgrade -ConfigFile deploy/profiles/full-local-sandbox.json -NonInteractive -Target services
```

Platform API、Trigger Gateway、Workflow Coordinator、Workflow Worker、Trace Writer 和 Web 镜像已重新构建、导入并滚动升级。`platform-api-migrate` 与 `trace-writer-migrate` Job 成功，`_sqlx_migrations` 中 Version 24 `resource grant requests` 和 Version 25 `department resource grant review` 均为 `success=1`。升级后 Web 与 `/health/live` 返回 200，Platform API `/health/ready` 报告 MySQL、Redis、ClickHouse 和 Object Storage 全部 Ready，未认证访问 Resource Options API 返回 401；全部 Deployment/StatefulSet 为 `1/1 Ready`，且不存在 `agentx-e2e` Namespace。
