# 阶段 11：n8n 式 Workflow Studio

## 1. 目标与用户价值

在真实引擎、节点协议和资源授权稳定后，提供接近 n8n 的拖拽、配置、表达式、部分执行、Pin Data、版本和发布体验，使用户不需要编写 JSON 即可完成 Workflow 全生命周期。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：[阶段 08](08-workflow-runtime-core.md) 的 Definition/IR/Execution API、[阶段 09](09-checkpoint-wait-recovery.md) 的调试命令和 [阶段 10](10-agent-cubesandbox.md) 的节点参数稳定。
- 当前 React Flow 已能保存最小资源引用 Draft，但尚未具备运行、表达式、Pin Data、Undo/Redo 和完整 Node Registry。

## 3. 范围和不做内容

实现 Workflow Studio、节点面板、参数表单、表达式、自动保存、调试、版本和发布。不实现 n8n JSON 导入、n8n 全量节点、移动端画布、通用低代码页面或插件市场。

## 4. 前端状态和不变量

- React Flow State 只包含编辑和布局信息，通过 Serializer 转换为 Workflow Draft DTO。
- Node Type、Version、端口和参数来自 Node Registry，页面不得硬编码重复协议。
- Draft Server Revision 是保存并发权威；本地 Undo/Redo 不替代 Revision。
- Pin Data、Mock 和参数覆盖只属于 Draft 手动调试，发布校验必须拒绝泄漏到生产 Version。
- 运行结果按 execution_id、node_id、run_index、branch_index、iteration_index 展示。
- 资源选择器同时检查当前用户可见性和目标 Workflow Service Identity 的 Grant。

## 5. 数据和 API

扩展 Draft 数据：画布位置、视口、注释、Pin Data Artifact、编辑 Revision 和 Schema Version。运行协议仍只消费 Definition 中的业务字段。

使用现有 API：Draft、Revision、Version、Deployment、Node Definition、Resource Picker、Execution、Checkpoint 和 Trace；新增自动保存、Draft Diff、Pin Data 和调试命令 DTO，不新增独立运行路径。

## 6. UI 架构

`workflow-designer` 按 canvas、nodes、edges、panels、store、model 和 utils 分层：

- React Flow 负责拖动、缩放、框选、Handle、Edge、MiniMap 和 Viewport。
- Zustand 保存当前编辑状态；独立 History Store 管理 Undo/Redo。
- React Hook Form 和 Zod 驱动参数编辑；JSON Schema/UI Schema 生成节点表单。
- Monaco Editor 用于 Expression、Prompt 和 JSON。
- ELK.js 提供自动布局。
- TanStack Query 负责 Draft、Registry、资源、Execution 和 Trace 服务状态。

## 7. n8n 体验对照

首期对齐：节点拖拽、连接、节点搜索、参数配置、表达式上下文、整流运行、单节点运行、执行到/从节点、Pin Data、节点输入输出、错误定位、运行高亮、版本发布。

不承诺：n8n JSON 字段兼容、社区节点直接运行、全部快捷键和全部内置节点。对照表记录 Agentx 行为、n8n 参考行为和有意差异。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| STU-001 | planned | RUN-001–005 | 前端 Workflow Draft Model、Serializer 和反序列化 | React Flow 往返不丢业务字段，纯 UI 字段不进入 IR |
| STU-002 | planned | RUN-002、AGT-013 | Node Registry Client、节点搜索和分类面板 | 版本、端口、图标和参数都来自 Registry |
| STU-003 | planned | STU-001–002 | 统一 Node、Handle、Edge 和连接类型 | main/error/AI 连接视觉明确且非法连接被拒绝 |
| STU-004 | planned | STU-002 | JSON Schema 参数表单和 UI Schema 组件映射 | 必填、条件字段、数组、Credential 和资源字段一致校验 |
| STU-005 | planned | RUN-004、STU-004 | Monaco Expression Editor、上下文和自动完成 | 表达式前后端校验结果一致，Secret 不可预览 |
| STU-006 | planned | RES-007、STU-004 | Model/MCP Tool/Skill/RAG/Memory/Credential 资源选择器 | 只展示用户可见且 Workflow 已授权资源 |
| STU-007 | planned | WCP-002、STU-001 | 自动保存、Revision 冲突、离开保护和恢复 | 并发编辑不静默覆盖，离线内容可恢复或明确丢弃 |
| STU-008 | planned | STU-001、STU-007 | Undo/Redo、复制粘贴、多选、快捷键和自动布局 | History 不包含服务端查询状态，跨 Workflow 粘贴重新校验资源 |
| STU-009 | planned | RUN-014、STU-001 | Run Workflow、Stop 和运行状态订阅 | 画布高亮来自真实 Execution Event |
| STU-010 | planned | REC-003、STU-009 | Execute Node、To Node、From Node | 缺失上游数据时提供 Pin、Checkpoint 或明确错误 |
| STU-011 | planned | REC-001–003、STU-010 | Pin Data、Mock 和节点输入输出面板 | Pin/Mock 只作用手动执行，发布时被检测 |
| STU-012 | planned | OBS-006、REC-009、STU-009 | Trace、Checkpoint、Fork 和错误定位入口 | 可从画布定位 runIndex/iterationIndex 和失败 Attempt |
| STU-013 | planned | WCP-004–007、STU-007 | Draft Diff、Version、发布、回滚和资源校验 | 发布生成不可变 Version，失败原因定位到节点和字段 |
| STU-014 | planned | STU-001–013 | n8n 行为对照、性能和桌面端验收 | 关键体验对齐且有意差异有文档和测试 |

## 9. 失败、安全和并发边界

- 自动保存失败显示未同步状态并保留本地内容，不能显示虚假“已保存”。
- Server Revision 冲突要求合并、复制为新 Draft 或明确覆盖确认。
- 跨 Workflow 复制不复制 Credential 明文或隐式 Grant。
- 执行事件断线后通过 Execution Query 校准，不依赖丢失的 SSE 消息。
- 大型节点结果按需加载 Artifact，不将全部 Trace 放入 Zustand。
- 发布按钮不能绕过后端编译、Pin/Mock、资源授权和预算校验。

## 10. 测试

- Serializer、History、连接校验、表单映射和表达式上下文单元测试。
- Draft Revision 并发、离线恢复和资源选择器集成测试。
- React Flow 拖拽、连线、复制、布局和键盘交互测试。
- Run/Stop、Node/To/From、Pin、Trace、Checkpoint 和发布端到端测试。
- 1280×800、1440×900、1920×1080 的中英文、浅深主题视觉检查。
- 大型 Workflow 节点数、事件量和 Artifact 按需加载性能测试。

## 11. 验收门禁

- 用户能只通过画布创建、配置、调试、版本化和发布 Workflow。
- React Flow State 不成为后端运行协议。
- 手动调试显示真实节点输入、输出、Attempt、Trace 和 Checkpoint。
- Pin、Mock 和参数覆盖不会进入生产 Version。
- n8n 关键语义与体验对照通过，差异有明确记录。

## 12. 对后续阶段的稳定输出

- 完整 Workflow Studio 和 Draft Serializer。
- Node Registry 驱动的参数与资源配置。
- 真实运行、调试、Trace、Checkpoint、版本和发布 UI。
- n8n 体验对照与差异清单。
