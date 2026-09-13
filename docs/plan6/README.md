# plan6：Workflow 画布插件与统一节点扩展

状态：功能已实现。2026-09-13 的执行边界修复与当前验收结果见 `evidence/p6-11-runtime-boundaries.md`：SDK/RPC 2、调用级进程隔离、真实并发、权威动态契约、流式文件和 Trace 降级。P6-10 与 P6-09 保留为历史证据。

## 1. 目标

用户能够在企业门户的“资源 → 画布插件”中导入经过审批的插件包，管理版本与可用状态，并在 Workflow 中使用插件提供的节点、React 交互界面、TypeScript/Node.js 执行逻辑和 Trace 展示。插件作者通过官方 SDK、可运行模板和根目录 `AGENTS.md`，可以让 AI 完成开发、验证与打包。

内置节点与用户插件共用节点定义、Catalog、UI 注册、版本解析和诊断扩展入口。调度、Lease/Fencing、Checkpoint、恢复、审批等待和 Agent 内核保持 Rust 权威。

P6-10 记录了 79 项任务与 40 项场景的历史基线；本次复核发现的缺口、修复和明确测试范围以 P6-11 为准，不能将历史通过记录扩大为当前所有边界均已验证。

## 2. 阅读顺序

| 文档 | 内容 |
|---|---|
| [01-product-and-management.md](01-product-and-management.md) | 菜单、导入、版本、启停、引用、删除、权限与 API |
| [02-architecture-and-contracts.md](02-architecture-and-contracts.md) | 节点包、权威来源、Definition/IR、数据分域、设计时调用与发布 |
| [03-ui-sdk-and-builtin-nodes.md](03-ui-sdk-and-builtin-nodes.md) | 动态 React、公共 SDK、内置节点抽取与编辑契约 |
| [04-node-runtime-and-trace.md](04-node-runtime-and-trace.md) | Rust 调用 Node.js、双向协议、执行治理、Trace 数据与展示 |
| [05-developer-template.md](05-developer-template.md) | AI 开发模板、AGENTS.md 内容、开发命令与协议测试 |
| [implementation-plan.md](implementation-plan.md) | 阶段、任务、依赖、代码落点、删除清单和完成门禁 |
| [contracts.md](contracts.md) | 已实现的协议版本、包锁、Runner RPC与Trace字段 |
| [06-e2e-acceptance.md](06-e2e-acceptance.md) | 自动化场景、故障测试、真实浏览器操作、性能和清理证据 |

## 3. 已确定的设计决策

1. 插件代码按经过审批且可信处理；使用同页面 React 组件和真正的 Node.js 运行环境，不在本期增加市场审核系统、签名信任平台或强沙箱建设。
2. 首期用户插件仅支持 TypeScript/Node.js；TS 在构建时检查并编译，运行时加载 JS 制品。浏览器产物与服务端产物分别构建。
3. 一个包可包含多个节点。节点定义、UI、执行入口和 Trace 扩展一起形成不可变版本；Workflow 精确锁定包版本及制品摘要。
4. Rust Worker 通过 Tokio 为每次业务调用管理独立 Node Runner 进程；设计时调用由 Runtime Gateway 使用独立容量执行。调用终止时回收进程树与目录，源码缓存按摘要复用。不新增 Plugin Daemon，也不要求每个插件维护 HTTP 服务或 Kubernetes Deployment。
5. Catalog 对内置和安装包使用同一套解析规则；节点业务 Manifest 以构建产物为权威。Rust 原生执行器表只绑定核心实现，不重复维护 UI/参数定义。
6. Set、List、HTTP 作为本期完整 TypeScript 内置插件；其余内置节点先统一包定义与 UI，核心执行能力按明确矩阵保留 Rust。必须删除已完成切换的旧执行和 UI 分支。
7. 参数、InputBinding、端口、输出 Schema、Item/Lineage、Artifact、资源引用和有效输出契约继续由平台统一理解；自定义 UI 不削弱后端保存与编译校验。
8. 平台自动生成节点/Attempt 诊断；插件可以增加子 Span 和版本化内容。时间线与标准数据视图属于平台，插件只扩展业务内容展示。
9. 默认停用只阻止新使用和变更后的重新发布；已有正式 Deployment 与已创建 Execution 继续使用冻结制品。紧急停止使用已有执行取消/Deployment 停用能力，不能让“停用插件”产生隐式批量取消。
10. 插件更新不会自动改写 Workflow。删除受草稿、版本、Deployment、Execution 和保留期内 Trace 引用保护；历史可读性是制品保留规则，不是兼容代码层。
11. Portal 正式验收仅覆盖 Web 桌面端，复用现有组件、主题、布局与中英文本地化。
12. 不做旧数据迁移、双读、别名、shim 或兼容包装。切换只影响本任务相关内容，不清理无关工作树或环境。

## 4. 非目标

- 不兼容 Dify 插件包、n8n 社区节点或它们的 Workflow JSON。
- 不开放用户自定义 Coordinator、Loop 调度器、持久等待状态机、Trigger 生命周期或 Agent 内核替换。
- 不为插件增加任意写数据库、改 Workflow Draft、创建 Attempt 的内部接口。
- 不建设在线插件商城、在线编辑完整插件源码、远程 Git/npm URL 安装、自动更新、插件间依赖管理或多语言运行器。
- 本期 npm 依赖限可随 JS 制品打包的依赖和 Node 内置模块；原生 `.node` 扩展、系统二进制依赖和安装脚本要求在打包时明确拒绝，不能运行时尝试联网补装。
- 基础 UI 组件和 Trace 引擎不是可卸载插件；它们是稳定宿主能力。

## 5. 当前基线与架构变化

实施前重新核对 [当前架构](../02-system-architecture.md)、[节点协议](../11-node-integration.md)、[Definition 契约](../12-workflow-5.md) 和 [plan5](../plan5/implementation-plan.md)。当前代码的重要限制：

| 当前入口 | 当前事实 | plan6 变化 |
|---|---|---|
| `src/crates/agentx-runtime/src/registry.rs` | 公开类型边界，装载并校验内置包 Manifest | `src/plugins/builtin/*` 包 Catalog + 核心执行绑定 |
| `src/crates/agentx-runtime/src/compiler.rs` | 依赖白名单与原生节点契约推导 | 包锁、通用节点契约、核心语义解析器 |
| `src/services/platform-control/src/catalog_api.rs` | 数据库快照只能对应 Registry 来源 | 内置包和租户安装包共同解析 |
| `src/web/src/features/workflow-designer/panels/node-inspector.tsx` | 固定 `ACTION_PANELS` | 统一插件组件注册 |
| `src/services/agentx-v2-runtime/src/worker_runtime.rs` | 固定 capability 执行分派 | 增加通用 Node.js 插件 capability |
| `src/crates/agentx-runtime-contracts/src/query.rs` | Trace Span/Content 固定枚举 | 固定插件承载类型 + 命名空间业务内容 |
| `src/web/src/app/navigation.ts` | 无画布插件入口 | 资源菜单增加“画布插件” |

plan6 已形成外部可执行节点扩展面，并为 Worker 引入固定 Node.js 运行依赖。P6-00 记录协议版本切换，`docs/02、04、06、07、09、10、11、12、13` 已同步当前实现；最终边界与证据以 P6-10 为准。

## 6. 参考依据

以下依据已在方案讨论中核对；实施冻结依赖时须再次检查官方接口与目标版本。结论是复用其成熟模式，不复制其协议。

- [Dify Tool Plugin](https://docs.dify.ai/en/develop-plugin/dev-guides-and-walkthroughs/tool-plugin)：工具描述、参数和 Python 执行分离。
- [Dify Plugin Daemon](https://github.com/langgenius/dify-plugin-daemon)：本地插件子进程及标准流通信。
- [n8n Node UI](https://docs.n8n.io/connect/create-nodes/build-your-node/reference/node-ui-elements)：标准节点 UI 基于预置控件，不能等同于任意 React/Vue 组件入口。
- [n8n 节点包加载器](https://github.com/n8n-io/n8n/blob/master/packages/core/src/nodes-loader/package-directory-loader.ts)：安装包、节点描述和执行类注册。
- [Tokio Process](https://docs.rs/tokio/latest/tokio/process/)：异步启动、标准流与进程生命周期。
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification)：请求、响应、通知；分帧及业务取消需要另行约定。
- [OpenTelemetry JS](https://opentelemetry.io/docs/languages/js/instrumentation/)：活动上下文、嵌套 Span、事件和异常。

## 7. 总完成条件

- [x] 管理员从“画布插件”真实上传包，普通 Workflow 开发者无需接触部署与 Runner 参数即可使用。
- [x] 不重建 Web/Rust 镜像即可安装新的业务插件；冷 Worker 可以仅从 Runtime 制品启动节点。
- [x] 全部现有节点 UI 共用扩展入口；Set/List/HTTP 完整使用公开 TypeScript SDK；核心节点原有行为不回退。
- [x] Trace 自动接入、实时子 Span、业务展示、历史版本、缺失诊断和成本归属均有证据。
- [x] 一个脱离 Agentx 仓库的开发目录仅凭模板与 SDK 就能完成插件开发、检查、打包和导入。
- [x] 相关静态检查、契约、组件测试、真实 Node Runner 和临时 Kubernetes E2E 全部通过。
- [x] 临时 Namespace、进程与测试容器完成清理；常驻开发 Deployment 副本恢复；完成报告列明未测的平台与边界。
