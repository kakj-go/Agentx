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
    ├── tools/
    ├── skills/
    ├── knowledge/
    ├── memory/
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

推荐目录：

    workflow-designer/
    ├── canvas/
    ├── nodes/
    ├── edges/
    ├── panels/
    ├── store/
    ├── model/
    └── utils/

## 8. 画布状态转换

React Flow 的 Node 和 Edge 结构只属于前端编辑状态，不能直接成为后端运行协议。

    React Flow State
          ↓ serialize
    Workflow Draft DTO
          ↓ backend compile
    Workflow IR
          ↓ execute
    Execution State

转换层负责：

- 删除纯 UI 字段
- 固化 Node Type Version
- 将 Handle 映射为端口
- 将 Edge 映射为连接类型
- 校验资源连接
- 保留画布位置用于再次编辑

## 9. 画布扩展

- ELK.js 用于自动布局。
- Zustand 保存编辑器状态。
- 独立 History Store 管理 Undo 和 Redo。
- Monaco Editor 用于 Expression、Prompt 和 JSON。
- 大型 Workflow 后续通过按需渲染、结果引用和面板虚拟化控制性能。

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
- /tools
- /skills
- /knowledge
- /memory
- /organization
- /roles
- /workflows/:workflowId/editor

Playground 调用正式 Application API，不建立单独的执行实现。

首期企业工作台只验收 1280px 及以上桌面布局。Sidebar 折叠状态保存到 agentx.sidebar.collapsed；不实现移动端 Drawer。一级列表页统一使用本地搜索、状态筛选、8 条分页和无结果状态。
