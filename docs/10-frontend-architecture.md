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

画布视觉由 `uiSchema.canvas.role` 驱动（`default`、`trigger`、`branch`、`flow`、`merge`、`loop`、`suspend`、`approval`、`sub_workflow`、`agent`、`code`、`error_handler`），前端再映射为 `compact`、`agent`、`attachment`、`editor` 四个视觉族。执行节点统一使用 n8n 式紧凑矩形几何：普通节点为 96×96、Agent 为 224×96、图标区为 48×48；不再按角色绘制菱形、箭头、六边形或不对称轮廓。角色语义由图标、Trigger 标记、端口标签、边框和运行状态表达。Handle 外层为 16×16、可见标记约 10px、命中区至少 24px，React Flow `connectionRadius=60`；全部执行输出位于右侧，普通输出按 Manifest 顺序在上，Error 输出统一排在其下方。资源附件仍由 `editorKind=binding` 表示，Agent 的 Model、Tool、Memory、Knowledge、Skill 端口位于底边。便签和分组是仅编辑态派生节点，只进入 Editor Document。空画布只显示添加 Trigger 和搜索节点两个入口。

连接交互使用独立状态机维护 source hover、connecting、compatible、incompatible、occupied、committed 和 cancelled；合法性通过增量 GraphIndex 校验方向、类型、基数、自连接、重复边和动态 Handle。非 variadic 输入已占用时以一个历史事务替换旧边；存在多个兼容输入时必须显式选择端口。执行边、Binding 边和 Error 边分别使用实线箭头、无箭头虚线和危险色虚线箭头，反向主连接使用固定底部间距、水平偏移和圆角路由。Edge Hover/选中工具栏只提供插入和删除，重新连接通过 React Flow 边端点拖拽完成。

画布不保留常驻节点栏；圆形 `+`、空画布入口和端口快速添加均打开同一个 Node Creator。正常缩放下，未占用的执行输出和资源槽默认展示 `+`；已占用的非 `variadic`/非 `multiple` 端口不再展示，低于 0.65 缩放时统一隐藏。创建器负责搜索、合法端口过滤和大预览，Manifest 分组默认仅展开第一个，其他分组按需折叠；便签和分组命令位于创建器顶部。单击执行节点或附件立即打开 480px Details，单击空画布清除选择并关闭；关闭按钮和 Esc 只关闭 Details 并保留选择。

Manifest `localizations` 是节点名称、说明、搜索关键词、端口和 Binding Slot 展示名的唯一业务本地化来源。显示按当前语言、`en-US`、基础字段依次回退，协议 ID 不翻译。Workflow Settings 的 `primaryOutputNodeId` 进入 Definition 和 Undo/Redo；主要输出在节点表面稳定标识，不写入 Editor Document。

## 8. 画布状态转换

React Flow 的 Node 和 Edge 结构只属于前端编辑状态，不能直接成为后端运行协议。Serializer 同时产出运行 Definition 和纯 UI Editor Document，Debug Overlay/运行高亮走独立模型。

    React Flow State
          ├── serialize definition ──> Workflow Definition 3.0 ──> IR
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

## 9. 画布扩展

- ELK.js 用于自动布局。
- Zustand 保存编辑器状态。
- History slice 以手势事务管理 Undo 和 Redo；拖动开始记录参与实体旧位置、结束只提交这些实体的一个 Patch 命令。
- Monaco Editor 用于 Expression、Prompt 和 JSON。
- 大型 Workflow 在 150 节点启用可见元素渲染、300 节点隐藏 MiniMap；缩放低于 0.65 隐藏标签，低于 0.35 使用简化内容。`IncrementalGraphIndex` 稳定复用 `nodeById`、`portByHandle`、源/目标端口边索引和 Binding 摘要 Map，只更新结构、端口或边的变化项；位置帧不重建连接索引。Group、Node 和 Edge 视图索引保持无关对象引用稳定，节点运行态只替换自身与关联边。
- 自动保存使用 Server Revision；Undo/Redo 只改变本地 Editor State，不能回退服务端 Revision。
- Execution Event 使用 Cursor 重连，断线后通过 Execution Query 校准；非终态运行更新最多每 100ms 合并一次，终态立即提交；完整 Trace 和大型输出不进入 Zustand。

## 10. 路由

一级路由与企业工作台菜单保持一致：

- /
- /workflows
- /applications
- /playground
- /executions
- /approvals
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

Playground 调用正式 Application API，不建立单独的执行实现。

首期企业工作台只验收 1280px 及以上桌面布局。Sidebar 折叠状态保存到 agentx.sidebar.collapsed；不实现移动端 Drawer。一级列表页统一使用本地搜索、状态筛选、8 条分页和无结果状态。

## 11. M2.1 资源界面

- Model 详情使用统一 Dialog 编辑 Provider、Alias 和 Deployment Revision，并展示不可变历史。
- Credential 详情允许修改名称和状态，但绝不把 Secret 放入表单默认值或 Query Cache。
- MCP 列表和详情替代旧 Tool 页面，Tool Schema 使用只读 JSON Schema 字段树展示名称、类型、必填、说明、约束及嵌套关系，策略和受控调试使用独立 Dialog。
- Skill 详情采用文件树、编辑/预览区和元数据区三栏布局；根 `SKILL.md` 在正文上方独立编辑必填描述并隐藏 YAML frontmatter 细节，Markdown 正文使用 MIT 许可的 MDXEditor/Lexical 富文本层并继续保存标准 Markdown，上传和 ZIP 导入继续走统一 API Client。
- 资源详情页不嵌入授权面板；`/resource-grants` 使用资源类型 Tab 分栏聚合 Credential、Model、MCP Server、MCP Tool、Skill、Knowledge 和 Memory，每栏列出对应资源并统一处理 Department 与 Workflow Service Identity Grant。
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
