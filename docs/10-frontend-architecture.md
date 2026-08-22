# 前端架构与 Workflow 画布

## 1. 技术栈

- React
- TypeScript
- Vite
- Tailwind CSS
- Radix UI
- Class Variance Authority
- React Router
- TanStack Query
- TanStack Table
- Zustand
- React Hook Form
- Zod
- i18next
- Lucide React
- React Flow

Tailwind 是唯一的视觉样式体系，不再引入 Ant Design 或另一套完整组件库。Radix UI 只提供无障碍行为和交互原语，最终视觉由 Agentx Token 和 Tailwind Class 决定。

## 2. 企业工作台

正式布局使用企业工作台方案：

    EnterpriseLayout
    ├── TenantSwitcher
    ├── Sidebar
    │   ├── 工作空间
    │   ├── 运行与质量
    │   ├── 资源
    │   └── 组织
    ├── Header
    │   ├── Breadcrumb
    │   ├── GlobalSearch
    │   ├── ThemeSwitch
    │   ├── Notification
    │   └── UserMenu
    └── PageContent

固定规格：

- Sidebar 展开 248px
- Sidebar 折叠 72px
- Header 64px
- 页面间距 24px 或 32px
- 基础控件高度 36px
- 基础圆角 8px
- 页面容器最大宽度 1480px

## 3. 设计令牌

颜色使用语义名称：

- background
- surface
- sidebar
- foreground
- muted
- muted-foreground
- border
- primary
- success
- warning
- danger
- canvas

业务代码不得用具体灰度或品牌色替代语义 Token。浅色和深色通过同一组语义 Token 切换。

主题偏好包含 system、light 和 dark。浏览器保存用户选择，未设置时跟随操作系统；初始化脚本必须在 React 挂载前解析主题，避免浅深色闪烁。

字体使用本地系统字体栈，不依赖外部字体 CDN。英文优先 Segoe UI，中文依次使用 PingFang SC、Microsoft YaHei 和 Noto Sans SC。

## 4. UI 组件分层

shared/ui 保存 Button、Input、Select、Dialog、Dropdown、Tooltip、Table、Form、Badge、Tabs、Drawer 和 Toast 等基础组件。

shared/components 保存 PageContainer、PageHeader、FilterBar、DataTable、MetricCard、StatusBadge、EmptyState 和 ErrorState 等页面组合组件。

Feature 页面优先组合共享组件，不重复定义页面标题、表格密度、状态颜色和空状态。

`apps/web/src/docs` 集中保存面向最终用户的内嵌接入文档、双语内容、动态代码示例和文档展示组件；Feature 页面只传入当前实体、公开 Endpoint 与已发布 Schema，不在业务页面复制长篇说明。Application 的 API Key 与 Webhook 文档以当前激活 Deployment 为数据源，在尚未创建 API Key 或 Webhook 时也必须能直接查看输入/输出 Schema；未创建 Webhook 时只用占位符表示尚未生成的专属 URL 和 Secret。接口参考按 Endpoint 展示请求字段、响应字段和 Curl、Java、Go、Node.js、Python 示例。外部 API 的方法、路径、Header 和 Schema 仍以版本化 OpenAPI 为契约真相，内嵌文档负责快速开始、安全说明和错误排查，不替代 OpenAPI。

## 5. Feature 边界

    features/
    ├── workflows/
    ├── workflow-designer/
    ├── applications/
    ├── executions/
    ├── approvals/
    ├── datasets/
    ├── evaluations/
    ├── models/
    ├── mcp/
    ├── skills/
    ├── knowledge/
    ├── memory/
    ├── resource-grants/
    └── organization/

Feature 不得引用其他 Feature 的内部文件。共享逻辑进入 shared，稳定的业务协议通过 Feature 公共入口暴露。

Mock 数据属于对应 Feature，只用于基础界面展示和本地筛选，不能进入 shared/ui，也不能模拟服务端权威状态或完整 CRUD。

## 6. 国际化

- 首期支持 zh-CN 和 en-US，默认回退到 zh-CN。
- 已保存语言优先于浏览器语言，语言偏好保存到 agentx.locale。
- 菜单、页面、状态、提示和演示数据全部使用翻译资源，不在页面中维护成对文案。
- 语言切换后同步更新 html lang。
- 菜单配置只保存翻译 Key，Sidebar、面包屑和全局搜索共享同一份配置。

## 7. React Flow

Workflow 画布使用 React Flow，不自行实现拖动、缩放、框选、Handle、Edge、MiniMap 和 Viewport。

Agentx 自己实现：

- 节点业务组件
- main、error 和 AI 资源连接
- 节点配置面板
- 连线合法性校验
- Workflow Draft 序列化
- 自动保存
- Undo 和 Redo
- 运行状态高亮
- Pin Data
- Checkpoint 和部分执行入口
- Node Catalog/Manifest 表单渲染
- Draft Revision 调试快照入口

推荐目录：

    workflow-designer/
    ├── canvas/
    ├── nodes/
    ├── edges/
    ├── panels/
    ├── forms/
    ├── api/
    ├── store/
    ├── model/
    └── utils/

Studio 采用 n8n 风格的紧凑桌面画布：画布左上角仅保留圆形 `+`，按需展开 320px Node Creator；中央为完整宽度的无限 Canvas，单击节点后打开 480px 全高 Node Details，底部为可折叠的全局 Execution Rail。节点类型、端口、参数和能力全部来自 Node Manifest；业务组件不得维护 Agent/Code 等 Node Type 白名单。Feature 内任何文件不得超过 2000 行。

画布视觉由 `uiSchema.canvas.role` 驱动（`default`、`trigger`、`branch`、`flow`、`merge`、`loop`、`suspend`、`approval`、`sub_workflow`、`agent`、`code`、`error_handler`），前端再映射为 `compact`、`agent`、`attachment`、`editor` 四个视觉族。执行节点统一使用 n8n 式紧凑矩形几何：普通节点为 96×96、Agent 为 224×96、图标区为 48×48；不再按角色绘制菱形、箭头、六边形或不对称轮廓。角色语义由图标、Trigger 标记、端口标签、边框和运行状态表达。Handle 外层为 16×16、可见标记约 10px、命中区至少 24px，React Flow `connectionRadius=60`；全部执行输出位于右侧，普通输出按 Manifest 顺序在上，Error 输出统一排在其下方。资源附件仍由 `editorKind=binding` 表示，Agent 的 Model、Tool、Memory、Knowledge、Skill 端口位于底边；Skill 和 Tool 只从附件分组创建并连接 Agent，不出现在 AI 执行节点分组。便签和分组是仅编辑态派生节点，只进入 Editor Document。空画布只显示添加 Trigger 和搜索节点两个入口。

连接交互使用独立状态机维护 source hover、connecting、compatible、incompatible、occupied、committed 和 cancelled；合法性通过增量 GraphIndex 校验方向、类型、基数、自连接、重复边和动态 Handle。非 variadic 输入已占用时以一个历史事务替换旧边；存在多个兼容输入时必须显式选择端口。执行边、Binding 边和 Error 边分别使用实线箭头、无箭头虚线和危险色虚线箭头，反向主连接使用固定底部间距、水平偏移和圆角路由。Edge Hover/选中工具栏只提供插入和删除，重新连接通过 React Flow 边端点拖拽完成。

画布不保留常驻节点栏；圆形 `+`、空画布入口和端口快速添加均打开同一个 Node Creator。正常缩放下，未占用的执行输出和资源槽默认展示 `+`；已占用的非 `variadic`/非 `multiple` 端口不再展示，低于 0.65 缩放时统一隐藏。创建器负责搜索、合法端口过滤和大预览，Manifest 分组默认仅展开第一个，其他分组按需折叠；便签和分组命令位于创建器顶部。单击执行节点或附件立即打开 480px Details，单击空画布清除选择并关闭；关闭按钮和 Esc 只关闭 Details 并保留选择。

Manifest `localizations` 是节点名称、说明、搜索关键词、端口和 Binding Slot 展示名的唯一业务本地化来源。显示按当前语言、`en-US`、基础字段依次回退，协议 ID 不翻译。Start/End 是固定边界节点，Start 配置 Inputs/Contexts，End 配置 Workflow 正式 Outputs；两者不进入 Catalog，也不能删除。

画布资源字段统一使用基于 Radix Popover 的 `ResourcePicker`。资源选项 Query Key 包含 Workflow、Resource Type 和 Operation，前端遍历后端分页得到全部当前可见资源，再提供即时搜索；同一类型的不同操作不得复用错误的授权状态。资源行的选择按钮和行尾动作按钮是两个独立交互目标。未授权行不可选择，但可以直接“授权”或“申请”；Pending 提供申请详情入口，Rejected 提供重新申请。存在 Pending 项时每 5 秒轮询，窗口重新聚焦时强制刷新。直接授权或审批完成只失效并刷新 TanStack Query，不调用 `onChange`，用户必须重新打开 Picker 手动选择。已选资源撤权、禁用或失去可见性时保留原引用，显示脱敏危险状态，并由统一资源校验阻止保存、创建版本和运行。

## 8. 画布状态转换

React Flow 的 Node 和 Edge 结构只属于前端编辑状态，不能直接成为后端运行协议。Serializer 同时产出运行 Definition 和纯 UI Editor Document，Debug Overlay/运行高亮走独立模型。

    React Flow State
          ├── serialize definition ──> Workflow Definition 5.0 ──> IR
          └── serialize editor ──────> Editor Document

    Pin / Mock / Runtime Highlight ──> Debug Overlay / Execution State

转换层负责：

- 删除纯 UI 字段
- 固化 Node Type Version
- 将 Handle 映射为端口
- 将 Edge 映射为连接类型和稳定 Connection Order
- 校验资源连接
- 将位置、视口、注释和分组只写入 Editor Document

服务端 Draft Revision 原子保存 Definition 和 Editor Document。TanStack Query 保存服务端权威数据；Zustand Editor Store 按 Document、Interaction、History 三个逻辑 slice 维护规范化编辑状态；History 以实体 ID 保存节点、边、便签和分组的前后 Patch，不保存视口、选择、Hover、临时连线或运行高亮；Runtime Overlay Store 按 executionId 隔离运行状态和结果。四者不得互相复制成为第二权威。

所有声明 `x-agentx-dynamicValue` 的 Input、Textarea、Prompt、Expression、JSON 和 Mapper 叶子共用 Reference Picker。文本类字段使用 Lexical Token Editor，将普通文本与变量 Chip 作为独立节点混排，支持光标、选区、Backspace/Delete、Undo/Redo、中英文 IME，以及携带 Agentx 自定义 MIME 的复制粘贴；UI 不展示协议字符串。Expression 使用递归 Visual Builder 编辑 Literal、Reference、Unary/Binary、Conditional、Call、Array 和 Object AST，不向普通 Studio 暴露 CEL/Monaco 源码。

引用树只读取 Start Inputs、声明 Context 和当前节点可达前置节点的 Effective Output Contract。选择器以 Node ID 持久化，以节点名称、端口和字段路径显示；节点重命名不修改 Selector。运行历史只能提供非权威样例值。敏感 Context 不允许插入普通字段或 Preview。

## 9. 画布扩展

- ELK.js 用于自动布局。
- Zustand 保存编辑器状态。
- History slice 以手势事务管理 Undo 和 Redo；拖动开始记录参与实体旧位置、结束只提交这些实体的一个 Patch 命令。
- Monaco Editor 只用于 Code；Prompt/Template 使用 Lexical Token Editor，Expression 使用 Visual Builder，JSON/Mapper 使用结构化叶子绑定控件。
- 大型 Workflow 在 150 节点启用可见元素渲染、300 节点隐藏 MiniMap；缩放低于 0.65 隐藏标签，低于 0.35 使用简化内容。`IncrementalGraphIndex` 稳定复用 `nodeById`、`portByHandle`、源/目标端口边索引和 Binding 摘要 Map，只更新结构、端口或边的变化项；位置帧不重建连接索引。Group、Node 和 Edge 视图索引保持无关对象引用稳定，节点运行态只替换自身与关联边。
- 自动保存使用 Server Revision；Undo/Redo 只改变本地 Editor State，不能回退服务端 Revision。
- Execution Event 使用 Cursor 重连，断线后通过 Execution Query 校准；非终态运行更新最多每 100ms 合并一次，终态立即提交；完整 Trace 和大型输出不进入 Zustand。

Workflow Studio 底部 Trace、独立执行详情和 Node Inspector 复用 `features/traces` 中的 `TraceWorkspace/TraceNodeDetails`。Workspace 默认进入节点视图：顶部固定显示最终输出、状态、耗时、Token 和成本，主体按 Start Boundary、每个 Node Execution、End Boundary 排列；Node 卡片以 Runtime MySQL 的输入输出为权威，按 Port/Item 解包并优先展示稳定语义字段，内部 Span 默认折叠且只在节点展开时携带 `nodeExecutionId` 加载。Trace 延迟或不可用必须显示明确诊断状态，不能用空输入替代权威数据。三个入口必须向共享内容组件传递同一个按 Execution 作用域授权的 Artifact 下载动作；超过 Trace 内联预算的内容只展示脱敏 Preview 和 Artifact 入口，浏览器不得绕过 `/executions/{executionId}/artifacts/{artifactId}` 直接访问对象存储。

高级调用瀑布复用分页 Query Hook、`TraceWaterfall` 和 `TraceDetail`。Span 数据只保存在 TanStack Query Cache，不复制到 Zustand。瀑布保留筛选匹配项的祖先上下文，支持层级折叠、类型/异常筛选、搜索、50%–200% 时间轴缩放、键盘 treegrid 操作和继续加载；默认选择首个失败 Span，其次运行中 Span，最后选择 Execution 根 Span。详情按 Span 类型与 `contentKind` 动态生成页签，纯生命周期 Span 不显示误导性的空输入/输出。运行中宽度按当前时间刷新，终态只使用服务端耗时。

桌面端高级详情列固定约 370px，低于 1120px 后移到瀑布下方。Studio 首次进入 Trace 页签时，执行轨道扩展到最多 560px（同时受视口 80% 上限约束），但不缩小用户手动设置的更大高度；用户继续向上拖动时最高可占视口的 4/5。轨道自身裁剪溢出，内部节点视图、瀑布和详情各自滚动，避免详情遮挡画布或相邻面板。选择 Node 或子 Span 时联动画布节点。Node Inspector 直接复用单节点语义详情，不再自行搜索 Span 后套通用详情。独立执行详情使用 Trace 和 Recovery 两个主视图并默认显示 Trace；Recovery 只保留 Checkpoint、Wait、Approval、Side Effect、事件与 Fork，删除重复的节点 Outline 和输入输出面板。

## 10. 路由

一级路由与企业工作台菜单保持一致：

- /
- /workflows
- /applications
- /playground
- /executions
- /approvals
- /approvals/resource-grants/:id
- /datasets
- /evaluations
- /models
- /mcp
- /skills
- /knowledge
- /memory
- /resource-grants
- /organization
- /roles
- /workflows/:workflowId/editor

Playground 调用正式 Application API，不建立单独的执行实现。页面用 `mode=parameters|conversation`、`applicationId` 和 `sessionId` 恢复刷新前状态，运行与消息历史全部来自服务端。

参数测试采用“无 Session 运行历史 / Schema 表单 / 结果详情”三栏布局。共享 `SchemaForm` 负责 string、multiline、enum、number、integer、boolean、object、array、Artifact、Artifact Array、递归默认值、恢复默认和 JSON 模式，Workflow Studio 的 Run Parameters 与 Playground 复用该组件。选择历史运行时，表单以当前 Deployment Schema 默认值为基础，只回填 Execution 输入中名称与类型仍兼容的字段，忽略旧版本已删除或改型的参数。Artifact 先上传到 `/gateway/v1/artifacts`，随后用 Artifact Reference 调用正式 Invocation API；结果面板展示输出、错误、耗时、Token、成本、文件和 Execution/Trace 入口。

对话测试采用“会话历史 / 消息历史与固定 Composer”两栏布局，工作区填满页面剩余高度，各栏内部独立滚动。共享映射配置放在 Radix Dialog 中，由带 Tooltip 和可访问名称的齿轮图标打开，只列出类型兼容的顶层字段；没有活动映射时发送按钮禁用，附件按钮在未映射文件输入时禁用。Composer 使用多行矩形输入框，附件 `+` 在左下角，发送图标在右下角，Enter 发送且 Shift+Enter 换行。`application:manage` 可以保存或清除映射，只有 `application:invoke` 的用户可以使用已发布映射。保存后轮询 `publishing/active/failed`，发布期间保留服务端草稿并暂停发送；用户与 Assistant 附件都渲染文件卡片，Assistant 消息提供 Execution/Trace 入口。Session 列表使用首条用户问题作为标题，单行省略并在悬停时展示完整标题。

首期企业工作台只验收 1280px 及以上桌面布局。Sidebar 折叠状态保存到 agentx.sidebar.collapsed；不实现移动端 Drawer。普通一级列表页继续使用本地搜索、状态筛选、8 条分页和无结果状态；执行记录页是受控服务端表格的明确例外，不能加载固定 100 条后本地筛选。

执行记录页的搜索、状态、时间和更多筛选写入 URL，Cursor 只保存在当前页面内存中。TanStack Query Key 使用规范化筛选条件和当前 Cursor；文本输入 300ms 防抖，任何筛选变化清空 Cursor 栈。公共查询统一使用 `limit`，页面固定为 8，通过 Cursor 栈实现上一页/下一页；Cursor 过期时回到第一页并明确提示结果已刷新。应用、Workflow、MCP Tool、用户和部门选项使用共享的可搜索多选 Popover 按需加载，其中部门使用 `/departments/search`，避免改变组织树接口语义；已选条件显示可移除标签与“清除全部”。

执行表格展示当前 Control 名称补充与 Runtime 审计快照的不同语义：应用和 Workflow 名称是 BFF 对当页稳定 ID 批量解析的当前名称；发起用户、发起部门和触发名称来自执行时快照。未知触发枚举保留协议原值用于诊断，已知值必须使用 `executions.triggerTypes` 的中英文映射。

## 11. M2.1 资源界面

- Model 列表只提供“新建模型”，在同一个 Dialog 中按连接、模型能力、所属部门和默认参数的顺序完成配置，不要求预先创建连接。Model 详情使用相同字段集合统一编辑；连接或模型参数变化都创建不可变 Deployment Revision，并展示 Revision 历史。
- Credential 详情允许修改名称和状态，但绝不把 Secret 放入表单默认值或 Query Cache。
- MCP 列表和详情替代旧 Tool 页面，Tool Schema 使用只读 JSON Schema 字段树展示名称、类型、必填、说明、约束及嵌套关系，策略和受控调试使用独立 Dialog。
- Skill 详情采用文件树、编辑/预览区和元数据区三栏布局；根 `SKILL.md` 在正文上方使用多行文本框独立编辑必填描述并隐藏 YAML frontmatter 细节。Markdown 正文使用 MIT 许可的 MDXEditor/Lexical 富文本层并继续保存标准 Markdown，工作区文件引用从富文本工具栏选择并插入当前光标位置；上传和 ZIP 导入继续走统一 API Client。
- 资源详情页不嵌入授权面板；`/resource-grants` 使用资源类型 Tab 分栏聚合 Credential、Model、MCP Server、MCP Tool、Skill、Knowledge 和 Memory，每栏列出对应资源并统一处理 Department 与 Workflow Service Identity Grant。Workflow Studio 允许对当前工作流的完整资源依赖包发起直接授权或审批申请，这是该统一授权控制面的任务内快捷入口。
- Model 列表展示与当前 Alias/Deployment 配置绑定的 `untested/healthy/unhealthy` 连接状态，连接测试从 Alias 详情页手动触发。
- Dialog 默认按内容完整展开并垂直居中；字段超过六项的通用表单自动使用双列宽布局，只有内容真实超过 `100dvh - 32px` 时才允许外层滚动。
- Workflow 最小画布通过 Serializer 保存 MCP Tool 资源引用，运行按钮在 Runtime 接入前保持禁用。

## 12. 前置条件交互

- 权限允许但缺少业务依赖时，操作按钮保持可点击，并通过共享 `PrerequisiteAction` 展示缺少项、已完成项和下一步入口。
- 只有请求加载中、正在提交或防止重复操作时可以直接禁用按钮；禁止使用无说明的灰色按钮表达“尚未创建依赖数据”。
- 缺少依赖且用户有创建权限时提供准确业务跳转；无创建权限时提示联系管理员，不能把“无权限”和“无数据”混为一谈。
- 依赖查询失败必须显示错误与重试，不能按空数组处理；Select 无可用选项时使用相同依赖说明。
- Application、Deployment、Evaluation、Workflow 发布、Dataset 发布、Skill 启用和 Model 编辑统一遵守该规则。
- 跳转创建依赖后应保留来源页面；返回时重新查询依赖并恢复原操作上下文。

## 13. 界面 E2E 门禁

业务 E2E 位于 apps/e2e，使用 1440×900 Chromium 和临时 Kubernetes Namespace。业务数据必须通过可见按钮和表单创建；API 与数据库只能准备环境、清理现场和校验证据。新增或修改业务按钮时必须补充真实点击测试，危险操作同时覆盖取消和确认，中英文与浅深主题纳入阶段验收。

## 14. 业务域国际化

翻译资源按 `common`、`navigation`、`auth`、`workflows`、`studio`、`applications`、`executions`、`approvals`、`notifications`、`datasets`、`evaluations`、`runtime`、`credentials`、`models`、`mcp`、`skills`、`knowledge`、`memory`、`sandbox`、`resourceGrants`、`organization`、`roles` 和 `errors` 业务域拆分。禁止新增 `m2/m3/m4/m5/m7` 或跨业务 `pages` 顶层命名空间。

`common.*` 只保存保存、取消、删除、状态、分页、确认等跨业务含义完全一致的文案。别名、连接、版本发布、轮换等词进入所属业务域；Model 使用 `models.fields.modelName` 和 `models.fields.upstreamModelId`，Skill 使用 `skills.fields.name/alias/description`，避免同名参数跨模块串词。中英文资源必须保持相同键结构，语言探测、`agentx.locale`、`html lang` 和稳定错误码翻译保持不变。

中文界面采用以下业务术语：Workflow 为“工作流”、Skill 为“技能”、Memory 为“记忆”、Agent 为“智能体”、Execution 为“执行”、Dataset 为“测试集”、Case 为“用例”、Deployment 为“部署”、Environment 为“环境”、Checkpoint 为“检查点”、Service Identity 为“运行身份”。Workflow Studio 使用“工作流设计器”，Playground 使用“调试台”。组合术语统一使用“工作流版本”“技能版本”“记忆命名空间”“工作流执行”“节点执行”“派生执行”“测试集版本”“测试用例”和“工作流运行身份”。

`Revision` 与不可变 `Version` 必须按语义区分：Draft Revision 为“草稿修订号”，Workspace Revision 为“工作区修订号”，Model Deployment Revision 为“部署配置版本”，其他计数型 Revision 为“修订号”；不可变发布物才称为“版本”。`Trace`、`Runtime`、`Worker`、`Artifact`、`Definition`、`Diff`、`Schema`、`Endpoint`、`Runner`、`Token` 保留英文，Agentx、MCP、API、HTTP、JSON、MIME、URI、UUID、OpenSandbox、Mem0、SKILL.md 等标准名或产品名同样不翻译。技术词与中文组合时采用自然语序，例如“智能体运行记录”“Runtime 调用”“本次执行的 Artifact”“评测 Trace”。

稳定接口枚举必须在所属业务域建立翻译映射，普通业务区域不得裸显 `active`、`manual_upgrade`、`resource_missing_or_disabled` 等协议值；未知枚举统一显示“未知（原始值）”并保留诊断信息。协议事件名、字段名、ID、Hash 和 JSON 只允许在明确的技术调试区域原样显示。日期、时间和数字统一通过共享 locale 格式函数按 `i18n.resolvedLanguage` 渲染，非法日期显示 `—`。用户录入的资源名、节点自定义名、代码和协议值不自动翻译；Manifest 文案按当前语言、`en-US`、基础字段顺序回退。

## 15. 列表安全删除

所有支持物理删除的实体只在列表或子对象列表使用共享 `EntityDeleteButton` 和 `EntityDeleteDialog`。列表行操作统一显示“删除”文字按钮，不使用仅图标按钮；按钮保留 ARIA 标签和危险色，禁用时通过 Tooltip 说明原因。删除确认不使用 `window.confirm`，父实体详情页头不增加删除入口。无 `*:delete` 权限时隐藏；根部门、内置角色和内置环境显示禁用按钮及原因。

Dialog 打开后分页加载删除影响。无引用时显示不可撤销确认；有引用时按模块展示来源名称、关系、节点位置和跳转入口并禁用确认。删除期间禁止关闭和重复提交；成功后刷新对应 Query。若 DELETE 因竞态返回 409，直接以错误 `details` 中最新的 `DeletionImpactResponse` 刷新当前 Dialog。

父实体详情页不放置父实体删除入口。Webhook 和 Schedule 当前没有独立列表路由，因此其删除按钮仅作为 Application 详情页内的子对象列表操作；按钮仍使用相同权限、预检和事务复检契约，Application 本身只能从 `/applications` 列表删除。

## 16. 表单字段错误

共享表单和非共享业务表单统一解析 `ApiClientError.detail.fieldErrors`。字段错误显示在对应控件下，设置 `aria-invalid`/`aria-describedby`，失败后聚焦首个错误字段；编辑时只清除当前字段错误。已知错误码使用本地化文案，未知安全 4xx 使用服务端公开 message，5xx 使用通用文案并显示 requestId。
