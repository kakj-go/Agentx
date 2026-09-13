# P6-08 完成状态复核

状态：历史审计。本文记录 2026-09-06 整改前的 43/79 基线；列出的 36 项缺口已全部关闭，当前结果见 [`p6-10-final-remediation.md`](p6-10-final-remediation.md)。

复核日期：2026-09-06。

## 1. 结论

plan6 尚未全部完成。首个插件的导入、动态 React UI、画布保存、Rust/Node 执行、基础 Trace、版本启停和模板下载已经形成可运行闭环，完整 Kubernetes 回归也曾通过；但首轮 `p6-07.md` 把基础闭环和既有通用测试扩大解释成了全部 79 项任务与 40 项场景。

本次按“任务中的每个子句均已实现，并有与该行为直接对应的自动化证据”重新检查，结果为：

| 阶段 | 完整 | 未完整 | 结论 |
|---|---:|---:|---|
| P6-00 | 6 | 3 | 核心协议已建立；管理 DTO 生成和黄金用例不足 |
| P6-01 | 10 | 0 | 最小端到端插件闭环完成 |
| P6-02 | 3 | 7 | 基础生命周期可用；管理体验、删除竞态、GC/缓存和隐藏入口验收未闭合 |
| P6-03 | 4 | 10 | 动态 UI 可加载；动态契约没有进入保存/编译，内置 core 仍是 Rust/固定前端注册 |
| P6-04 | 6 | 5 | 基础 Runner/宿主桥接可运行；异常进程治理、init、内存和运行基线不足 |
| P6-05 | 6 | 4 | 基础插件 Span/renderer 可用；大内容、故障、成本和复杂示例不足 |
| P6-06 | 4 | 3 | 模板可下载并独立构建；同源生成、仓库外变体和 CI 导入未完成 |
| P6-07 | 4 | 4 | 静态检查、旧业务回归和清理完成；最终覆盖与报告结论不成立 |
| **合计** | **43** | **36** | **不能标记为完成** |

未完整任务已经在 `implementation-plan.md` 中恢复为未勾选：

- P6-00-03/04/09。
- P6-02-01/04/05/06/07/08/09。
- P6-03-01/02/04/05/06/10/11/12/13/14。
- P6-04-07/08/09/10/11。
- P6-05-04/08/09/10。
- P6-06-04/06/07。
- P6-07-02/04/06/08。

## 2. 已确认完成的主链

- “资源 → 画布插件”菜单、列表、详情、ZIP 上传预览、确认安装、启停、默认版本、下载和基础引用保护可用。
- ZIP 校验、Control 对象保存、插件版本与节点 Catalog 注册、包摘要和 Work Package 冻结已经实现。
- 浏览器可加载包内 React ESM/CSS，面板、Canvas、Result 和 Trace renderer 的基础入口可运行。
- `agentx/data` 的 Set/List 和 `agentx/http` 的 HTTP 使用 Node Runner 执行；Rust Worker 通过 JSON-RPC 调用固定 Node.js 24.20.0。
- `ctx.http/model/credentials/artifacts` 基础宿主桥接、进程池、超时、Windows Job Object、Linux 进程组和基础 Outcome Unknown 已有实现。
- JSON Mapper 的页面导入、画布连线、配置、保存、执行、插件 Span、业务 Trace 内容、v1/v2、停用、下载和引用阻止删除已经通过真实浏览器链路。
- 模板 ZIP 可在仓库外目录完成 install/check/test/build/pack，产物摘要可复现。
- Run `327672be64` 的完整 `pytest tests/e2e` 为 22 项通过；其中 Playwright JUnit 共 29 个用例。Run `d2e1e118bd` 的最终 Control UI 聚焦回归为 1 项通过。

这些结果证明基础产品可用，但不能证明下列高级条件。

## 3. 阻塞完成的实现缺口

### 3.1 契约与管理 DTO

- Canvas Plugin 路由已经写入 Platform OpenAPI，但响应只有 description，没有请求/响应 Schema；生成的 `apps/web/src/shared/api/generated.ts` 中没有 Canvas Plugin DTO。页面继续使用 `apps/web/src/features/canvas-plugins/types.ts` 的手写类型。这不满足 P6-00-03/04 的单一生成契约要求。
- 包校验单元测试只有正确包、保留 package ID、未打包 import 和模板 ZIP；没有覆盖计划列出的错误 Schema/SDK、路径、入口、native dependency 和节点身份黄金矩阵。

### 3.2 管理生命周期与并发

- 管理页是列表加详情卡片，没有计划要求的详情/版本/使用情况/开发说明页签、来源/状态筛选和服务端分页交互。
- Studio 的版本切换只有一个 `window.confirm`，随后直接替换 `nodeType/typeVersion`；没有整包影响预览，也没有统一显示端口、边、引用和配置错误。
- 删除版本/包先在事务外查询引用，再开启删除事务。Workflow 可以在引用检查和删除之间并发保存，因此 P6-02-05/E2E-11 要求的删除竞态保护尚未实现。
- Import TTL 只在下一次上传时顺带清理。删除对象失败只写日志，现有 retention worker 不处理 Canvas Plugin orphan，因此补偿与定期 GC 未闭合。
- Runtime 没有插件包缓存/同摘要重新下载路径；执行直接使用 Work Package 内的 `runtimeSource`。这能运行，但没有实现 P6-02-08 描述的制品缓存、损坏重取和 Runtime OSS 故障语义。

### 3.3 动态契约与 Provider

- `resolveDefinition` 只有独立 HTTP 端点和 Runner 单元测试。Studio 不调用该端点，Draft 保存、发布预览和 Compiler 仍直接使用数据库中的静态 Manifest，因此动态端口和输出 Schema 没有成为权威 IR 契约。
- `resolveDefinition` 没有按输入 Hash 缓存。
- Provider Hook 只在面板加载时请求一次固定 URL，没有把当前参数/资源快照传入，也没有 UI 搜索、分页、取消和晚到响应抑制。设计时 Runner 只等待一个结果，不处理反向宿主调用，因此 Provider 无法调用凭据测试或字段发现宿主能力。

### 3.4 UI SDK 与内置节点

- Plugin Panel 把 `readOnly` 固定为 `false`；统一 Panel Context 没有只读状态。Result renderer 只接入 Studio 节点检查器，没有形成所有执行详情入口的统一出口。
- UI Host 只有 React、Field、Input、Button、Select、SmartInput；没有计划声明的 locale、主题状态、Portal、资产 URL 和设计时调用接口。
- `agentx/core` 当前只是 Rust Manifest 上附加的包身份。核心 Manifest 仍由 `crates/agentx-runtime/src/registry.rs::default_manifests` 构造，前端仍有固定 `NATIVE_PANELS` 表；没有实际 core 包产物和统一包注册。
- 动态端口、缺包、停用、版本切换影响、只读、复制粘贴、键盘和反复挂载资源回收没有完整的插件组件/浏览器测试矩阵。

### 3.5 Runner 治理

- Plugin 返回的 `retryable` 和 `details` 当前被 Rust Worker丢弃，没有进入确定失败/可重试决策或诊断。
- Runner 只清理自己的 deadline timer 和 active map；插件自己创建的 timer、订阅和模块全局状态会留在复用进程中，不满足跨 Attempt Context 清理要求。
- Worker 镜像入口直接是 Rust 服务，没有容器 init。Linux 进程组可以发送终止信号，但 PID 1 的孤儿/僵尸回收要求尚未闭合。
- Values 只有进程数量和空闲时间，没有 Node 单进程内存上限。没有冷/热装载、IPC、吞吐、长期内存和持续池运行的量化基线。
- Windows 有超时子进程树单元测试，但 Linux/Kubernetes 尚无插件崩溃、后台任务、Lease 丢失、迟到结果、Worker 替换和 Runtime 制品故障的专门测试。

### 3.6 Trace

- Plugin Span 数量限制为 64，但实时 content/event 对同一 Span 没有完整数量/深度预算。
- Plugin content 仅经过 `bounded_preview` 截断，没有像大 Runtime 内容一样外置为 Artifact。
- 没有插件自报计量来源字段和插件父子成本去重的直接实现/测试。
- 没有零埋点、同类型多内容、嵌套 HTTP/Model、并发 Promise 父子上下文和历史版本 renderer 的完整自动化。
- `trace-degraded.spec.ts` 存在，但没有被 `tests/e2e/product/test_playwright.py` 的 suites 调用，也没有 pytest 编排 ClickHouse 停机/恢复。因此 E2E-35 在 Run `327672be64` 中实际没有执行。

### 3.7 开发模板与 CI

- 模板中的 vendored SDK `index.d.ts` 是手写副本，没有与正式 SDK 同源生成或漂移检查。
- 仓库外 E2E 只构建原始模板，不修改成计划要求的字段重命名/过滤变体，也不通过页面上传该仓库外产物。
- 浏览器测试上传的是仓库内 `templates/canvas-plugin/dist` 产物。不存在计划文档所写的 `tests/e2e/product/test_canvas_plugins.py` 和 `tests/e2e/observability/test_plugin_trace.py`，也没有独立 CI 完成导出 ZIP→变体→导入链路。

## 4. 40 项场景复核

以完整场景的所有断言都存在为口径：

- 直接完整覆盖：E2E-06、E2E-20、E2E-29、E2E-36、E2E-40。
- 明确缺少系统级场景：E2E-10、E2E-12、E2E-13、E2E-19、E2E-32、E2E-35、E2E-37。
- 其余 28 项有部分实现或部分断言，但缺少场景中至少一个关键条件，不能标为完整通过。

典型扩大解释包括：

- E2E-03 的浏览器测试没有变量引用、Undo/Redo断言。
- E2E-04 执行并读取了插件节点输出，但没有配置并断言下游引用该输出。
- E2E-07 在一个 Workflow 中先后执行 v1/v2，没有证明两个 Workflow 同时运行两个版本。
- E2E-17 只直接调用静态 Provider API，没有 UI 搜索/分页/取消/晚到响应/凭据错误。
- E2E-30 在执行完成后查询 Span，没有证明运行结束前可实时查询。
- E2E-33 只在停用后读取冻结 Manifest，没有在停用/更新后重新打开历史 Trace renderer。
- E2E-38 只有三个既有 Studio 快照，没有画布插件管理页和插件 Trace 的中英/浅深主题与可访问性矩阵。
- E2E-39 的性能用例使用 Set 节点和 mocked Draft API，没有测动态插件 UI 反复挂载、Runner 长时间运行与缓存清理。

## 5. 后续完成顺序

1. 先把动态 `resolveDefinition` 接入 Studio、保存、发布和 Compiler，并冻结有效端口/Schema。
2. 修复删除竞态、后台 TTL/orphan GC、历史执行授权和 Runtime 制品缓存语义。
3. 补齐 UI Host Context、只读/版本影响预览和真实 `agentx/core` 注册来源。
4. 完成 Runner Context 清理、容器 init、单进程内存与 Linux/Kubernetes 故障治理。
5. 完成 Trace 大内容 Artifact、预算、成本和历史 renderer。
6. 实现仓库外模板变体并上传同一产物，补齐缺失 pytest 文件和 40 项逐场景故障编排。
7. 重新运行聚焦插件 E2E 和完整 `pytest tests/e2e`，只有全部场景都有直接证据后再恢复完成状态。
