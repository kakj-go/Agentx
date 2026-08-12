# M6 Workflow Studio 验收证据

状态：`done`。最终验收日期：2026-08-08。最终 Kubernetes Run ID：`20260808T051133152Z`。

M6 的 STU-001～016 已全部实现并通过契约、单元、构建、临时 Kubernetes 和真实浏览器闭环验收。Studio 使用 Agentx 原生 Definition 4.0、Editor Document、Debug Overlay、Node Manifest 和 ExecutionRuntime；不兼容 n8n JSON、节点包、Credential 或 JavaScript 表达式。

## 1. 已验收能力

- Definition 4.0 不含坐标，Connection 使用显式 `order`；Editor Document 和 Debug Overlay 不进入 Compiler/IR。
- Draft 与 Version 分别保存 Definition/Editor Hash；Draft Debug 固化指定 Revision 的 Definition、IR、Manifest、资源、授权和 Debug Plan Snapshot。
- Catalog、Compiler 和 Studio 共用 Rust Registry Reconcile 后的 Manifest 与 Hash；前端不存在业务 NodeKind 白名单。
- React Flow Studio 支持 Palette 搜索/拖拽、main/error/AI Handle、框选/多选、复制粘贴、对齐、ELK 自动布局、Undo/Redo、自动保存、离线恢复和显式 Revision Conflict。
- 画布采用紧凑 n8n 风格：普通节点统一 96×96、Agent 224×96、Handle 外层 16×16；Manifest 角色通过紧凑视觉族、图标、端口和状态表达，不再使用角色异形。Node Creator 可折叠，Node Details 为 480px 全高四 Tab 视图，Runtime Panel 为可折叠全局执行轨道；Sticky Note 和 Group 作为 Editor Document 编辑态元素保存。
- Editor Store 按 Document/Interaction/History slice 隔离；历史使用实体 ID 前后 Patch，拖动 1000 节点图中的一个节点只记录该节点，Viewport、选择、Hover 和临时连接不进入 Undo。
- `IncrementalGraphIndex` 稳定复用 Node/Port/Source Edge/Target Edge/Binding Summary Map；位置帧不更新索引，Group、Node 与 Edge 缓存使无关对象保持引用稳定。非终态 Runtime 更新最多每 100ms 合并，终态立即刷新。
- 连接状态机覆盖 source hover、兼容性、占用替换、动态 Handle、每源端口 Connection Order 和 Edge 重连；多个兼容输入必须由用户显式选择，不再静默连接第一个端口。
- 跨 Workflow 粘贴使用会话级剪贴板，重建 Node/Binding/Edge ID，并在目标 Workflow 通过 Draft Validate 重新校验节点版本、资源可见性和 Service Identity Grant。
- Inspector 支持 Text、Textarea、Number、Boolean、Select、Collection、Fixed Collection、Mapper、Resource/Credential、Provider、Expression、Prompt、JSON 和 Code；未知 UI 控件阻止保存。
- Agentx Expression 使用 Monaco 补全，Prompt 使用原生多行文本输入并共用 Reference Picker；Platform API 的同一个 Rust `ExpressionEngine` 做预览和权威校验，预览不读取 Credential，并递归脱敏 Secret/Token/Authorization 等字段。
- Full、Single、To、From、Stop、Pin、Mock、Input/Output、事件 Cursor、Attempt、Lineage、Trace、Checkpoint 和 Fork 均走真实 Runtime。
- Version Dialog 分离 Definition/Editor Diff，提供历史 Version 只读视图；发布/回滚使用不可变 Version，Debug Overlay 不进入 Version。
- 大图在 150 个节点启用可见区域渲染、250 个节点切换确定性线性复杂度布局、300 个节点隐藏 MiniMap；500/1000 节点 Playwright 基准同时验证首次交互、拖动/缩放 FPS、P95 输入延迟和可见节点裁剪。

本地 Builtin 数据处理能力共 17 项（全部为 Manifest v1、`Builtin` capability、无 Credential/Provider/外部网络依赖）：`filter`、`limit`、`sort`、`remove_duplicates`、`split_out`、`aggregate`、扩展 `merge`（Merge By Key）、`rename_fields`、`json_transform`、`no_op`、`stop_and_error`、`item_generator`、`date_time`、`base64`、`hash`、`compare_datasets`、`structured_validator`。

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
| STU-010 | done | Monaco Expression/JSON/Code、普通 Prompt 文本输入、Agentx 补全、Rust AST 校验和脱敏预览测试 |
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
- Web Oxlint、32 个 Vitest 文件共 95 项测试、TypeScript 和 Vite 生产构建通过。新增覆盖增量 GraphIndex、Patch History、动态 Handle、端口基数、占用替换、显式多输入选择、每源端口 Connection Order、重连事务、100ms Runtime 批处理、引用隔离和 n8n 反向边路由。
- Deployment Profile 测试和全部 Kustomize 渲染通过。
- 前后端源文件均低于 2000 行。
- Worker Builtin 测试覆盖正常、空输入、表达式、字段缺失、类型错误、Lineage 去重和 Error Port/正式失败；Runtime Catalog/Compiler 测试确认 17 项 Manifest 均可查询和编译。
- Studio 新增聚焦覆盖包括 Serializer、Connection、Patch History、Autosave、Provider、Mapper、跨 Workflow Clipboard、Version Diff、Debug Plan、Runtime Event、Overlay 和 500/1000 节点性能。

## 4. Kubernetes E2E

执行：

```powershell
.\scripts\e2e.ps1
```

最终完整运行使用 `-SkipBuild -KeepNamespace` 复用本轮已重建并导入的 Platform API、Web、Worker、Sandbox Manager 与 Fixture 镜像，从全新 Namespace 执行完整七阶段流程；成功后手动删除保留的诊断 Namespace 并再次核对 OpenSandbox 和开发副本。

| Stage | Tests | Failures | Skipped | Errors |
|---|---:|---:|---:|---:|
| m2.1-control-plane | 1 | 0 | 0 | 0 |
| m3-control-plane | 1 | 0 | 0 | 0 |
| m3-observability | 1 | 0 | 0 | 0 |
| m4-runtime-recovery | 2 | 0 | 0 | 0 |
| m5-agent-sandbox | 7 | 0 | 0 | 0 |
| m6-workflow-studio | 4 | 0 | 0 | 0 |
| m7-business-closure | 1 | 0 | 0 | 0 |
| 合计 | 17 | 0 | 0 | 0 |

最终 M6 性能基准（Playwright 合成 Draft，最终 Run ID）：

| 节点数 | 首次可交互 | 中位 FPS | P95 输入延迟 | 可见节点数 |
|---:|---:|---:|---:|---:|
| 500 | 509ms | 59.9 | 22.5ms | 77 |
| 1000 | 783ms | 59.9 | 28.5ms | 77 |

M6 浏览器用例通过 UI 创建 Workflow，使用节点创建器拖入 Agent、Code、Approval、Error Handler，并为 Agent 添加 Model 和 MCP Tool 附件，完成双语语义形状、边界 Handle、错误策略、主要输出、参数与资源配置、执行/AI/Error 连线、保存 Revision、Full/Single/To/From/Stop、事件断线恢复、Pin/Mock、Trace、Checkpoint/Fork、Version、Deployment 和并发冲突。Fixture 只准备账号、Model、MCP、Credential、Sandbox Profile 和 Approval 依赖，没有通过 API/SQL 写入被测 Workflow。

本地 Builtin 用例同样只通过 Studio UI 创建并运行三条 Workflow：13 节点数据变换链、Merge/Compare 多输入链、Validator/Stop Error 错误链；API 只读取 Execution 和 Node Run 结果作为断言证据。三条流程共同覆盖新增 16 项 Manifest 与扩展 Merge 的全部 17 项能力。

视觉证据覆盖 1280x800、1440x900、1920x1080，中文/英文和浅色/深色共 12 张截图；每个视口均断言页面无横向溢出和未翻译 key。

证据目录：

- `apps/e2e/test-results/kubernetes/20260808T051133152Z/`
- `m6-workflow-studio/junit.xml`
- `m6-workflow-studio/playwright-report/index.html`
- `m6-workflow-studio/artifacts/*/trace.zip`
- `m6-workflow-studio/artifacts/*/studio-*.png`、`local-transform-chain.png`、`merge-compare-chain.png`、`validator-stop-error-chain.png`
- `m6-workflow-studio/artifacts/*/workflow-canvas-performance.json`
- `m4-database-evidence.txt`、`m5-database-evidence.txt`、`m5-sandbox-cleanup.txt`（`created=0`、`remaining=0`）

最终清理已确认：`agentx-e2e` Namespace 不存在，OpenSandbox 没有本轮残留实例，清理证据为 `created=0`、`remaining=0`，开发 Namespace 原副本数已恢复。

## 5. M6 完成边界

- STU-001～016、阶段 11 和里程碑 M6 均为 `done`。
- M6 没有实现 n8n JSON、社区节点、Credential、JavaScript 表达式或移动端/实时协作兼容。
- M7 不再新增画布、节点配置、草稿调试或发布 UI；只消费 M6 冻结契约完成 Application/Trigger/Evaluation 等外围执行接线，以及配额、保留、生产 Sandbox RuntimeClass、Vault、供应链、跨租户安全、容量和发布门禁。
- AGT-010 生产强化仍按既定决定暂不重试，唯一归属为 M7 INT-006/010/011/014。
