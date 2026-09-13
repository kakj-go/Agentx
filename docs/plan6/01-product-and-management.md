# 画布插件菜单与管理流程

状态：部分完成。基础管理流程可用；页签/筛选、版本影响预览、删除竞态和后台补偿仍待完成，见 `evidence/p6-08-audit.md`。

## 1. 菜单与页面

在企业门户“资源”菜单下增加“画布插件”，放在 MCP 与 Skills 附近：

- 中文：画布插件；英文：Canvas Plugins。
- 路由：`/canvas-plugins`、`/canvas-plugins/:pluginId`。
- 导航权限：`canvas_plugin:view`；图标复用 Lucide 中符合现有导航风格的插件图标。
- 注册到全局搜索、面包屑、路由懒加载、权限守卫和两种语言资源。
- Studio 节点栏提供“管理画布插件”入口，进入同一个管理页面；没有管理权限时可查看已启用节点及开发说明，不出现不可操作的导入按钮。

沿用 `PageContainer/PageHeader/FilterBar/DataTable/StatusBadge/EmptyState` 和共享表单组件。主操作只有“导入插件”，次操作为“下载开发模板”。不在普通用户流程暴露进程池、RPC、入口文件和 Hash。

## 2. 列表和详情

列表展示：名称、简介、来源（内置/导入）、默认新建版本、节点数、状态、最近更新时间、操作。支持名称/包 ID 搜索、来源和状态筛选、现有风格的分页。

包详情使用四个页签：

| 页签 | 内容 |
|---|---|
| 概览 | 作者、简介、说明、当前默认版本、所含节点卡片 |
| 版本 | 每个不可变版本、校验结果、启停、设为默认、下载原包 |
| 使用情况 | 关联草稿、Workflow Version、Deployment 和保留制品的历史执行计数；有权限时可跳转 |
| 开发说明 | SDK 支持版本、模板下载、插件附带的开发文档；高级区域可见制品摘要和校验报告 |

列表和详情均覆盖加载、空数据、校验失败、服务错误、权限不足、版本冲突。内置包显示“随平台提供”；允许查看及下载开发示例，不提供卸载或任意替换内置制品的按钮。

## 3. 导入：上传、预览、确认

1. 点击“导入插件”，通过文件选择或拖拽上传一个 `.agentx-plugin` 文件；格式是 ZIP，扩展名仅为产品识别。
2. 系统校验包结构、协议/SDK、节点 ID、Schema、入口、依赖和摘要，展示插件名称、版本、节点列表与变更摘要。元数据来自包，不要求用户重复填写。
3. 校验成功后默认操作“导入并启用”；可选择“仅导入”。首次导入且启用的版本成为该包默认新建版本。
4. 同一包的新版本导入不改写旧 Workflow；设为默认仅影响后续从节点栏新建的节点。已打开画布收到 Catalog 更新后刷新可选目录，但不替换现有实例。
5. 失败停留在当前对话框，按文件/节点/字段显示错误；修正后重新上传。失败上传不得产生可见半安装版本或改写原默认版本。

插件审批在平台外完成，`canvas_plugin:manage` 的导入操作是本期可信入口。页面可展示“请导入已完成审批的插件”，不增加审批任务、审批人配置、强制签名或自动安全评分。

导入事务分为临时上传、不可变校验结果和确认安装。确认必须引用 `importId + bundleDigest`；不能再次按名称取文件。重复提交相同租户/包/版本/摘要幂等返回已有结果；相同版本不同摘要返回冲突。

## 4. 状态与版本语义

包版本仅有 `enabled/disabled` 可用状态；上传任务另有 `uploaded/validating/ready/failed/consumed/expired` 状态，不混作版本状态。来源、可用状态、默认版本是三个不同概念。

| 操作 | 新建/编辑/发布 | 已激活 Deployment 与执行 |
|---|---|---|
| 导入并启用 | 可选该版本；首次安装可成为默认 | 无变化 |
| 设为默认 | 以后新建选此版本 | 无变化 |
| 停用版本 | 节点栏不可新建；现有草稿仍可查看和保存，显示不可调试/发布的版本问题 | 冻结版本继续运行；不取消进行中的调用 |
| 重新启用 | 恢复新使用、调试与发布资格 | 无变化 |
| 切换草稿插件版本 | 显式选版本，对同包全部节点显示字段/端口影响；用户修复后才可发布 | 无变化 |
| 删除版本/卸载包 | 存在任何保留引用时拒绝，返回可见的引用摘要 | 不得造成依赖消失 |

停用默认版本时原子清空默认指针；不自动选择其他版本。仍启用的非默认版本可以在版本选择中使用。新的正式发布、Deployment 重新激活/回滚及新的 Draft Debug 均重新校验启用状态；已创建 Execution 的继续执行、重试与恢复不重新查询 Control 可用状态。历史 Fork 属于新执行：Control 对其冻结包重新检查可用资格，停用时拒绝新 Fork，不能换成最新版。

“切换插件版本”是显式重配置操作，一个 Workflow 的同一个包只锁定一个版本，因此同时处理该包的全部节点；不同 Workflow 可以使用不同版本。不提供参数迁移函数、字段别名或自动修复器。保留同名配置后按目标契约展示错误，用户确认形成一次可撤销编辑；未知字段不得偷偷通过保存校验。端口删除不能留下运行时悬空边；受影响连线和引用要在操作预览中列明。

## 5. 删除、引用与保留

- 版本不可覆盖。使用中的版本无法物理删除；删除按钮先显示引用检查结果，服务端在最终事务内再次校验。
- 引用至少覆盖 Draft Head、保留的 Draft Revision、Workflow Version、Deployment、Execution Snapshot 及保留期内的历史 UI/Trace 制品。
- 用户没有相关 Workflow 查看权限时仅看到不可删除原因和聚合数量，不泄露名称/参数。
- 卸载为删除所有未被引用的版本和包记录；有任一被引用版本则整个操作拒绝，不部分删除。
- Runtime 制品由现有对象保留/GC 链路根据依赖闭包清理；Control 删除成功不直接跨域删除 Runtime OSS。
- 不增加“强制删除依赖”“改用最新版继续运行”或缺包时执行空操作。
- 上传取消、过期和校验失败的临时对象由 TTL 清理；清理失败产生可观察重试，不污染正式安装状态。

## 6. 权限

| 权限 | 用途 |
|---|---|
| `canvas_plugin:view` | 管理页、包信息和有权限的使用情况 |
| `canvas_plugin:manage` | 导入、启停、默认版本、版本删除、包卸载、包下载 |
| 现有 `workflow:edit` | 在授权 Workflow 中使用已启用的租户插件 |
| 现有执行/Trace 权限 | 查看历史执行制品、业务诊断和 Artifact |

包在租户内共享，首期不复制一套部门插件授权系统。插件使用的 Credential/Model 等资源仍走现有资源授权和冻结依赖规则。普通设计者不因为调用插件获得包管理或原始凭据权限。

更新 `bootstrap_api.rs` 中权限种子与现有角色配置；页面隐藏/禁用只是交互，服务端必须校验相同权限。版本启停、默认切换和删除使用现有乐观锁模式；审计记录操作者、目标版本、摘要和动作，不记录包内业务数据。

## 7. Control API 草案

统一前缀 `/api/v1/canvas-plugins`，遵循当前 API 的错误、分页、时间和字段风格。

| 方法与路径 | 请求重点 | 响应重点 |
|---|---|---|
| `GET /canvas-plugins` | search、source、status、分页 | 包摘要、默认版本、节点数 |
| `POST /canvas-plugin-imports` | multipart 文件 | importId、状态、过期时间 |
| `GET /canvas-plugin-imports/{id}` | 上传任务 ID | 校验状态、摘要、元数据、字段问题 |
| `DELETE /canvas-plugin-imports/{id}` | 尚未确认的任务 | 取消结果，正式安装不受影响 |
| `POST /canvas-plugin-imports/{id}/install` | bundleDigest、enable、setDefault | 包/版本 ID、状态、幂等结果 |
| `GET /canvas-plugins/{id}` | 包 ID | 概览、版本摘要、节点定义 |
| `PATCH /canvas-plugins/{id}` | defaultVersionId、expectedRevision | 新 revision 与默认版本 |
| `PATCH /canvas-plugins/{id}/versions/{versionId}` | enabled、expectedRevision | 状态、受影响引用计数 |
| `GET /canvas-plugins/{id}/references` | versionId、分页 | 分类引用和可见目标 |
| `DELETE /canvas-plugins/{id}/versions/{versionId}` | expectedRevision | 删除结果或引用冲突 |
| `DELETE /canvas-plugins/{id}` | expectedRevision | 卸载结果或引用冲突 |
| `GET /canvas-plugins/{id}/versions/{versionId}/download` | 精确版本 | 原始包下载 |
| `GET /canvas-plugin-sdk/template` | 当前支持的 SDK 版本 | 官方模板 ZIP |

表中上传与模板使用同一 `/api/v1` 下的兄弟资源路径，不拼接在 `/canvas-plugins` 后。前缀与 DTO 在生成 OpenAPI 时只有一套最终定义。

错误至少覆盖：`PLUGIN_PACKAGE_INVALID`、`PLUGIN_SDK_UNSUPPORTED`、`PLUGIN_VERSION_CONFLICT`、`PLUGIN_NODE_ID_CONFLICT`、`PLUGIN_DISABLED`、`PLUGIN_VERSION_IN_USE`、`PLUGIN_ARTIFACT_MISSING`、`PLUGIN_REVISION_CONFLICT`。Schema 校验问题附 `path/code/message`；未知包、无权限等沿用当前错误规范。

## 8. 产品验收

- 每个新增按钮必须有真实浏览器点击路径；上传不得以直接写 OSS 或数据库代替。
- zh-CN/en-US、浅色/深色、1440×900 桌面均检查关键布局。
- 演示包含“导入 → 节点栏出现 → 拖入 → 自定义面板 → 保存重开 → 调试 → 发布 → Trace”。
- 更新、停用、删除的影响说明与实际执行规则一致；有引用时不存在误导性“已卸载”提示。
- 成功导入新节点不重建前端或 Rust 镜像；页面刷新/新浏览器会话仍能加载同一版本。
