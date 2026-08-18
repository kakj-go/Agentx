# Workflow 4.0 重构验收证据

状态：`done`。当前复核日期：2026-08-10。双终态、Studio 表单化、嵌套本地化、Projection Runtime 与 Workflow 4.0 错误闭环均已取得最新 Kubernetes 证据。

本文对应 [Workflow 4.0 契约](../12-workflow-4.md)，用于证明 Definition、Expression、Context、Composite Node、API/Chatbox、导入导出和 Studio 引用选择器已经形成同一条可执行链路。Workflow 4.0 明确不兼容 Definition 3.0、旧 Trigger 画布节点和旧表达式语法。

## 1. 实现边界

| 能力 | 实现位置 | 自动化证据 | 状态 |
|---|---|---|---|
| Definition 4.0 Start/End、节点 key、Inputs/Outputs/Contexts | `agentx-domain`、Workflow JSON Schema、Studio Serializer | Domain/Runtime 单测、Schema drift、M6 Studio E2E | done |
| Expression 2.0 AST、类型/基数/可达性/敏感字段校验 | `agentx-runtime` Compiler/Expression | `cargo test -p agentx-runtime --lib` | done |
| Execution/Session Context、Patch、CAS、Checkpoint | `agentx-infrastructure`、Coordinator | Infrastructure 单测、Workflow 4.0 E2E 和 MySQL 断言 | done |
| Composite Manifest、固定版本、Overlay、递归检测 | Platform API、Runtime Repository | Workflow 4.0 E2E | done |
| Start/End 面板与统一 Reference Picker | Workflow Studio | Web 组件测试、M6 Studio E2E | done |
| API、Chatbox、Multipart、Artifact、SSE | Trigger Gateway、Application Adapter | M7 与 Workflow 4.0 E2E | done |
| Workflow Package 签名、版本锁、资源重绑定 | Platform API | Package 单测、Workflow 4.0 E2E | done |

## 2. 二十项验收矩阵

| # | 场景 | 主要自动化证据 | 关键断言 |
|---:|---|---|---|
| 1 | Start 输入到 End 输出 | `m6-workflow-studio.spec.ts`、`workflow4-closure.spec.ts` | Definition 4.0 执行成功，正式结果只来自 End Outputs |
| 2 | Inputs、Outputs、Contexts 混合引用 | `workflow4-closure.spec.ts` | Parent Summary 同时解析三种命名空间，Context increment 为 1 |
| 3 | HTTP JSON 字段提取 | `m4-runtime.spec.ts`、`m7-business-closure.spec.ts` | Remote/HTTP Action 的结构化响应进入后继节点及 End |
| 4 | Model 文本 JSON Parse | `m5-agent-sandbox.spec.ts`、Runtime Compiler/Expression 单测 | Model 结构化结果遵循 Manifest Schema，解析失败进入明确错误路径 |
| 5 | Code 多返回值选择 | `m5-agent-sandbox.spec.ts` | Sandbox 结构化输出及 Artifact 可被 End 显式选择 |
| 6 | 文件输入和文件输出 | `workflow4-closure.spec.ts` | Multipart 文件转换为 Artifact Reference，End 返回受控元数据 |
| 7 | Context 在普通节点之间变化 | `workflow4-closure.spec.ts`、Infrastructure Context 单测 | Node Output、Context Patch、状态与 Checkpoint 同事务提交 |
| 8 | Context 在 Loop 中按轮次更新 | `m6-local-builtins.spec.ts`、Runtime Compiler/Context 单测 | Loop iteration 语义明确，Patch 幂等且不重复应用 |
| 9 | 并行 Context 冲突 | `workflow4-closure.spec.ts` | Session CAS 竞争只有一个提交，另一个明确失败 |
| 10 | Sub-workflow Context Overlay | `workflow4-closure.spec.ts` | Child base/final 三方合并，记录 `merge_overlay` Patch |
| 11 | Sub-workflow 失败回滚 Context | Runtime Repository/Context 单测、`workflow4-closure.spec.ts` 超时路径 | Child 非成功终态不提交 Overlay，Parent Context 不被污染 |
| 12 | Workflow 发布为节点后被父流程调用 | `workflow4-closure.spec.ts` | 发布版本生成 `workflow.<version-id>` Composite 类型并真实执行 |
| 13 | 固定子 Workflow 版本 | `workflow4-closure.spec.ts` | Child v2 发布后 Parent 仍返回固定 v1 结果 |
| 14 | API 同步调用 | `workflow4-closure.spec.ts`、`m7-business-closure.spec.ts` | `responseMode=sync` 返回最终 End Outputs |
| 15 | API 异步查询 | `m7-business-closure.spec.ts` | 202 Invocation 可查询到真实 Execution 终态 |
| 16 | SSE 流式输出 | `m7-business-closure.spec.ts` | Event Cursor 断线续传，最终保存完整响应 |
| 17 | Multipart 文字加文件 | `workflow4-closure.spec.ts` | 同一请求映射 question 与 attachments，文件内容不内嵌 Definition |
| 18 | Workflow 导出后导入另一个项目 | Package 单测、`workflow4-closure.spec.ts` | 签名、Manifest lock、Composite version 和 Definition 4.0 保持一致 |
| 19 | 外部资源缺失时阻止发布 | Package/Grant 单测、M2.1 Resource E2E | 映射必须同 Tenant、active 且固定版本有效 |
| 20 | Cancel、Timeout、Retry、Checkpoint Recovery | `workflow4-closure.spec.ts`、`m4-runtime.spec.ts`、`m4-recovery.spec.ts` | Parent 超时取消 Child；重试去重；Wait/Approval 仅恢复一次 |

## 3. Kubernetes 证据要求

完整验收命令：

```powershell
.\scripts\e2e.ps1
```

脚本必须创建全新的 `agentx-e2e` Namespace 和 PVC，执行 M2.1-M7 既有套件及 `workflow4-closure`，并额外断言：

- Parent Composite 只提交一条 `merge_overlay` Context Patch。
- Package 导入创建一份 Definition 4.0 Workflow。
- Parent timeout 后 Slow Child 进入 `cancelled`。
- Session CAS 失败节点记录 `SESSION_CONTEXT_VERSION_CONFLICT`。
- 未发布 Outbox、未投影 Runtime Event、活动 Sandbox Lease 和未撤销 Handle 均归零。

无论成功或失败，默认都必须删除临时 Namespace 并恢复开发 Namespace 原副本数。真实 Run 通过后，本文状态改为 `done`，记录 Run ID、JUnit/HTML/Trace 路径及 Namespace 清理结果。

## 4. 静态门禁

以下门禁必须在同一工作树通过：

```powershell
.\scripts\check.ps1
```

它覆盖 Rust format/clippy/workspace tests、Web lint/test/build、OpenAPI/JSON Schema/TypeScript 生成漂移、部署脚本测试、Kustomize render 和前后端单文件 2000 行限制。Workflow 4.0 不允许通过排除生成文件绕过行数门禁。

## 5. 最终验收记录

最终 Kubernetes 验收使用已由同一工作树构建并导入本地集群的镜像执行：

```powershell
.\scripts\e2e.ps1 -OnlySuite m7 -SkipBuild -Port 18081
```

- Run ID：`20260810T051551773Z`。
- M2.1 control plane 1 项、M6 Studio 4 项、M7 business closure 1 项、Workflow 4.0 closure 3 项全部通过；对应 JUnit 均为 `failures=0, skipped=0, errors=0`。
- [M2.1 JUnit](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/m2.1-control-plane/junit.xml)、[M6 JUnit](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/m6-workflow-studio/junit.xml)、[M7 JUnit](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/m7-business-closure/junit.xml)、[Workflow 4.0 JUnit](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/workflow4-closure/junit.xml) 和各自 HTML 报告已保留；通过用例均保留 Trace。
- M6 用例通过可见结构化表单完成 Start Inputs、Contexts、节点参数、Projection、Context Writes 和 End 成功/错误输出，并覆盖手工连线与 Start/End 布局持久化；配置过程不依赖原始 JSON 编辑入口。
- Workflow 4.0 用例确认 Projection 只写入表达式求值结果而不泄漏字段元数据，并覆盖 End.error `fail_fast`、双错误 `collect`、Composite Error 传播、Session Context CAS、Multipart、API、Package 和父子取消。
- [M7/Workflow 4.0 数据库证据](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/m7-database-evidence.txt) 记录 `workflow4PackageImports=1`、`workflow4ContextPatches=9`、`workflow4ChildExecutions=4`，并确认 Runtime Command、未发布 Outbox、未投影 Runtime Event、活动配额均为 0。
- [Sandbox 数据库证据](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/m5-database-evidence.txt) 确认活动 Lease、待投递 Runtime Call/Trace Outbox 和未撤销 Credential Handle 均为 0。
- `agentx-e2e` Namespace 已删除；`agentx` 中全部 Deployment/StatefulSet 恢复为 1 副本，并与 [测试前副本快照](../../apps/e2e/test-results/kubernetes/20260810T051551773Z/agentx-original-replicas.json) 一致。
- 同一工作树执行 `.\scripts\check.ps1` 成功，Rust、Web、生成物漂移、部署渲染和 2000 行限制均通过。

## 6. Start 字段类型设置专项复核

2026-08-12 使用独立临时 Namespace 对 Start 字段类型设置、输入物化与文件约束执行专项复核：

```powershell
.\scripts\e2e.ps1 -Namespace agentx-e2e-start-settings -OnlySuite start-input -KeepDevelopmentRunning -SkipBuild -Port 18085
```

- Run ID：`20260811T172715251Z`。
- [Start Input JUnit](../../apps/e2e/test-results/kubernetes/20260811T172715251Z/workflow4-start-input/junit.xml) 为 `tests=1, failures=0, skipped=0, errors=0`，HTML 报告与 Trace 位于同目录。
- 同一用例确认非法长度与空文件数组返回 `INPUT_SCHEMA_VALIDATION_FAILED`，递归默认值生成 `prefix=default-applied`，Multipart 文件转换为 Artifact Reference，并执行 MIME、单文件大小、总大小和数量约束。
- Composite Parent 固定调用 Child v1，发布与运行时均不再把 `workflow` 固定版本依赖误判为需要 Resource Grant 的外部资源。
- `agentx-e2e-start-settings` Namespace 已自动删除；`agentx` 中全部 Deployment/StatefulSet 保持 1 副本且 Ready。
