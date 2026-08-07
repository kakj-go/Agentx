# M6 Workflow Studio 验收证据

状态：`done`。最终验收日期：2026-08-07。最终 Kubernetes Run ID：`20260807T125227802Z`。

M6 的 STU-001～016 已全部实现并通过契约、单元、构建、临时 Kubernetes 和真实浏览器闭环验收。Studio 使用 Agentx 原生 Definition 3.0、Editor Document、Debug Overlay、Node Manifest 和 ExecutionRuntime；不兼容 n8n JSON、节点包、Credential 或 JavaScript 表达式。

## 1. 已验收能力

- Definition 3.0 不含坐标，Connection 使用显式 `order`；Editor Document 和 Debug Overlay 不进入 Compiler/IR。
- Draft 与 Version 分别保存 Definition/Editor Hash；Draft Debug 固化指定 Revision 的 Definition、IR、Manifest、资源、授权和 Debug Plan Snapshot。
- Catalog、Compiler 和 Studio 共用 Rust Registry Reconcile 后的 Manifest 与 Hash；前端不存在业务 NodeKind 白名单。
- React Flow Studio 支持 Palette 搜索/拖拽、main/error/AI Handle、框选/多选、复制粘贴、对齐、ELK 自动布局、Undo/Redo、自动保存、离线恢复和显式 Revision Conflict。
- 画布采用紧凑 n8n 风格：Manifest 视觉角色驱动异形节点，Node Creator 可折叠，Node Details 为 480px 全高四 Tab 视图，Runtime Panel 为可折叠全局执行轨道；Sticky Note 和 Group 作为 Editor Document 编辑态元素保存。
- 跨 Workflow 粘贴使用会话级剪贴板，重建 Node/Binding/Edge ID，并在目标 Workflow 通过 Draft Validate 重新校验节点版本、资源可见性和 Service Identity Grant。
- Inspector 支持 Text、Textarea、Number、Boolean、Select、Collection、Fixed Collection、Mapper、Resource/Credential、Provider、Expression、Prompt、JSON 和 Code；未知 UI 控件阻止保存。
- Agentx Expression 使用 Monaco 补全，由 Platform API 的同一个 Rust `ExpressionEngine` 做预览和权威校验；预览不读取 Credential，并递归脱敏 Secret/Token/Authorization 等字段。
- Full、Single、To、From、Stop、Pin、Mock、Input/Output、事件 Cursor、Attempt、Lineage、Trace、Checkpoint 和 Fork 均走真实 Runtime。
- Version Dialog 分离 Definition/Editor Diff，提供历史 Version 只读视图；发布/回滚使用不可变 Version，Debug Overlay 不进入 Version。
- 大图在 150 个节点启用可见区域渲染、250 个节点切换确定性线性复杂度布局、300 个节点隐藏 MiniMap；600 节点自动化基线要求 1.5 秒内完成且布局稳定。

## 2. STU 任务证据

| 任务 | 状态 | 主要实现与证据 |
|---|---|---|
| STU-001 | done | `agentx-domain/workflow.rs`、Definition Schema、Migration 0015、Editor/Overlay/Execution Source 字段 |
| STU-002 | done | `agentx-runtime/registry.rs`、`platform-api/catalog.rs`、Node Catalog OpenAPI 与 Manifest Hash |
| STU-003 | done | Application Runtime Port、runtime gRPC `oneof source`、Draft Revision Snapshot Repository |
| STU-004 | done | `model/serializer.ts` 往返测试，Definition 与 Editor 字段隔离 |
| STU-005 | done | Palette、通用 Manifest Node/Handle/Edge、Connection Validator 和 AI Attachment |
| STU-006 | done | Editor/History Store、会话剪贴板、目标 Workflow 复核、对齐、ELK 和快捷键测试 |
| STU-007 | done | 防抖保存、localStorage Recovery、离开保护和三选项 Revision Conflict 测试/E2E |
| STU-008 | done | Manifest JSON/UI Schema 表单、Collection/Fixed Collection/Mapper 和不支持控件门禁 |
| STU-009 | done | Model/MCP/Skill/RAG/Memory/Credential/Sandbox Selector、Grant 与递归依赖校验 |
| STU-010 | done | Monaco Expression/Prompt/JSON/Code、Agentx 补全、Rust AST 校验和脱敏预览测试 |
| STU-011 | done | Full/Single/To/From/Stop、精确 Revision 和可恢复 Event Cursor E2E |
| STU-012 | done | 独立 Overlay、Pin/Mock、可信输入来源、Artifact 和 Side Effect Decision E2E |
| STU-013 | done | 运行高亮、Attempt/Lineage、Agent/Sandbox Trace、Checkpoint/Fork E2E |
| STU-014 | done | Definition/Editor Diff、历史 Version 只读视图、发布/回滚和 Overlay 隔离断言 |
| STU-015 | done | 12 张中英文/浅深/三分辨率截图、键盘/焦点行为和 600 节点性能基线 |
| STU-016 | done | 临时 Kubernetes 中仅通过 UI 创建、配置、调试、版本化和发布 Workflow |

## 3. 静态与单元门禁

执行：

```powershell
.\scripts\check.ps1
git diff --check -- . ':!README.md'
```

结果：通过。

仓库级 `git diff --check` 仍会报告用户已有的 `README.md:66: new blank line at EOF`；该文件不属于本次画布重构，按工作树保护约束保留。本次涉及文件的差异检查无错误。

- Rust fmt、Clippy `-D warnings`、Workspace Tests 和 Doc Tests 通过。
- Platform/Gateway/Node OpenAPI、生成 TypeScript、Workflow/Manifest/Action JSON Schema 无漂移。
- Web Oxlint、29 个 Vitest 文件共 83 项测试、TypeScript 和 Vite 生产构建通过。
- Deployment Profile 测试和全部 Kustomize 渲染通过。
- 前后端源文件均低于 2000 行。
- Studio 新增聚焦覆盖包括 Serializer、Connection、History、Autosave、Provider、Mapper、跨 Workflow Clipboard、Version Diff、Debug Plan、Runtime Event、Overlay 和 600 节点布局。

## 4. Kubernetes E2E

执行：

```powershell
.\scripts\e2e.ps1
```

最终复跑使用 `-SkipBuild` 复用已构建的 Platform API、Web、Worker、Sandbox Manager 与 Fixture 镜像；Approval 主要输出恢复缺陷修复后单独重建并导入 Coordinator 镜像，再从全新 Namespace 执行完整七阶段流程。

| Stage | Tests | Failures | Skipped | Errors |
|---|---:|---:|---:|---:|
| m2.1-control-plane | 1 | 0 | 0 | 0 |
| m3-control-plane | 1 | 0 | 0 | 0 |
| m3-observability | 1 | 0 | 0 | 0 |
| m4-runtime-recovery | 2 | 0 | 0 | 0 |
| m5-agent-sandbox | 7 | 0 | 0 | 0 |
| m6-workflow-studio | 2 | 0 | 0 | 0 |
| m7-business-closure | 1 | 0 | 0 | 0 |
| 合计 | 15 | 0 | 0 | 0 |

M6 浏览器用例通过 UI 创建 Workflow，使用完整左栏和搜索面板拖入 Manual Trigger、Agent、Code、Approval、Error Handler、Model 和 MCP Tool，完成双语语义形状、边界 Handle、错误策略、主要输出、参数与资源配置、执行/AI/Error 连线、保存 Revision、Full/Single/To/From/Stop、事件断线恢复、Pin/Mock、Trace、Checkpoint/Fork、Version、Deployment 和并发冲突。Fixture 只准备账号、Model、MCP、Credential、Sandbox Profile 和 Approval 依赖，没有通过 API/SQL 写入被测 Workflow。

视觉证据覆盖 1280x800、1440x900、1920x1080，中文/英文和浅色/深色共 12 张截图；每个视口均断言页面无横向溢出和未翻译 key。

证据目录：

- `apps/e2e/test-results/kubernetes/20260807T125227802Z/`
- `m6-workflow-studio/junit.xml`
- `m6-workflow-studio/playwright-report/index.html`
- `m6-workflow-studio/artifacts/trace.zip`
- `m6-workflow-studio/artifacts/studio-*.png`
- `m4-database-evidence.txt`、`m5-database-evidence.txt`、`m5-sandbox-cleanup.txt`

默认清理已确认：`agentx-e2e` Namespace 不存在，OpenSandbox 没有本轮残留实例，本轮清理证据为 `remaining=0`，开发 Namespace 原副本数已恢复。

## 5. M6 完成边界

- STU-001～016、阶段 11 和里程碑 M6 均为 `done`。
- M6 没有实现 n8n JSON、社区节点、Credential、JavaScript 表达式或移动端/实时协作兼容。
- M7 不再新增画布、节点配置、草稿调试或发布 UI；只消费 M6 冻结契约完成 Application/Trigger/Evaluation 等外围执行接线，以及配额、保留、生产 Sandbox RuntimeClass、Vault、供应链、跨租户安全、容量和发布门禁。
- AGT-010 生产强化仍按既定决定暂不重试，唯一归属为 M7 INT-006/010/011/014。
