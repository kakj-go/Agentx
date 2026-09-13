# plan6 详细实施计划

状态：已完成。P6-10 严格复核已关闭上一轮发现的全部缺口；79/79 项任务、40/40 项 E2E 场景、完整 Kubernetes 回归和正式安装验收均已通过。

约束：先查看 Git 工作树；保护无关修改。前后端文件均不超过 2000 行。跨平台编排使用 Python，Kubernetes 系统测试只有 `pytest tests/e2e` 一个入口，部署复用 Rust agentxctl。旧实现完成替换后删除，不留兼容入口。

## 1. 阶段顺序与退出条件

| 阶段 | 范围 | 前置 | 退出条件 |
|---|---|---|---|
| P6-00 | 基线、架构、协议和契约冻结 | 阅读本目录与当前代码 | 唯一 Schema/版本、依赖选择和删除清单可评审 |
| P6-01 | 第一个完整插件垂直切片 | P6-00 | 真实导入 → 画布 UI → 保存 → Rust/Node 执行 → 标准 Trace → Kubernetes |
| P6-02 | 插件管理与发布生命周期 | P6-01 | 版本、启停、引用删除、冷 Worker/历史制品正确 |
| P6-03 | UI SDK、动态契约和内置节点抽取 | P6-01；版本切换依赖 P6-02 | 全内置 UI 统一；Set/List/HTTP 使用公开 SDK |
| P6-04 | 执行治理与部署完整性 | P6-01 | 超时、取消、进程树、幂等、故障恢复和资源预算通过 |
| P6-05 | 动态 Trace 数据与 UI | P6-02/P6-04；UI SDK 已可用 | 实时子 Span、业务视图、历史/缺失/乱序/成本正确 |
| P6-06 | SDK 模板、AGENTS.md 与 AI 开发体验 | P6-03/P6-05 | 独立目录开发、构建、打包、导入真实可用 |
| P6-07 | 全量系统验收与旧路径清理 | 所有前序阶段 | 相关检查、完整 E2E、证据、文档、清理全部闭合 |

P6-01 必须先跑出最小版本，不能等待所有管理页/Trace 高级能力完成。后续阶段可以按不冲突代码切片安排，但每个业务切片都包含前后端和相应测试；不能先拆掉全部内置节点后再等待插件系统成形。

## 2. P6-00：基线与协议冻结

- [x] P6-00-01 核对当前工作树、Node Registry、Compiler、Bundle、Worker、Trace 和菜单；记录基线提交及已存在修改。
- [x] P6-00-02 形成架构决策记录：包 Manifest 权威、原生执行绑定、Runtime 设计时操作、Worker Node 进程、Trace 扩展。
- [x] P6-00-03 冻结 NodePackageManifest/Reference、包锁、UI 导出、RPC、节点 Result、Trace 业务内容、管理 API DTO。
- [x] P6-00-04 冻结受影响协议的新版本常量与拒绝规则，扩展现有 generate-contracts/OpenAPI/TS 生成；不复制手写 DTO。
- [x] P6-00-05 确定 Node LTS/镜像、pnpm/TS、SDK 版本和 ESM 构建方案；验证 React 工厂共享依赖可独立装载。
- [x] P6-00-06 复用已存在的 zip、Tokio、Serde、JSON Schema、对象存储；评估 JSON-RPC/OTel 所需维护良好的库，并记录新增依赖的必要性。
- [x] P6-00-07 冻结 Control 表、Runtime 设计时请求、包引用/GC、初始化 DDL 和生成物变化。
- [x] P6-00-08 冻结管理产品规则：菜单位置、导入确认、默认版本、停用、Fork/重新激活、引用保护与权限。
- [x] P6-00-09 为协议准备黄金用例：正确包、错误 Schema/SDK、同版本不同摘要、前后端依赖混入、节点身份冲突。

落点：`src/crates/agentx-node-protocol`、`src/crates/agentx-domain`、`src/crates/agentx-runtime-contracts`、`src/crates/agentx-bundle-builder`、`contracts/schemas/runtime-v1`、`openapi`、`deploy/migrations/control`、`deploy/migrations/runtime`、`pnpm-workspace.yaml`。

证据：新增 `docs/plan6/evidence/p6-00.md` 与版本/契约记录；文件在实际完成时生成。门禁是协议闭合且可生成，不要求此阶段假造运行成功。

## 3. P6-01：第一个端到端插件

- [x] P6-01-01 建立 plugin-sdk、plugin-ui、plugin-runner 最小包，提供一个静态输入输出契约的 JSON 字段映射插件。
- [x] P6-01-02 新增“资源 → 画布插件”导航、权限、路由、列表空态与真实文件上传/导入确认；本阶段就能通过页面安装。
- [x] P6-01-03 实现 ZIP/Manifest 校验、不可变对象保存、包版本和节点 Catalog 注册，事务失败不半安装。
- [x] P6-01-04 实现精确包锁、普通插件节点编译、Bundle/Work Package 制品投递和 Runtime-local 校验。
- [x] P6-01-05 Web 动态装载真实构建产物，支持 panel/canvas、配置更新、变量绑定、自动保存和重新打开。
- [x] P6-01-06 Worker 加入 plugin_nodejs capability；Runner 握手、node.execute、结果/schema 校验、基本 deadline/进程清理。
- [x] P6-01-07 接入现有 Node/Attempt 自动 Trace 与只读输入输出，插件零埋点也可诊断。
- [x] P6-01-08 Worker 镜像加入固定 Node/Runner，最小 Helm/Values 配置可部署；不等最终阶段才第一次验证容器。
- [x] P6-01-09 增加 Playwright 导入/拖拽/编辑/重开/运行/Trace 测试，并由 pytest 在临时 namespace 调用。
- [x] P6-01-10 验证新浏览器会话和冷 Worker 没有源码目录/开发服务器仍可运行；证明新增插件无需重建镜像。

落点：`src/web/src/app/navigation.ts`、`src/web/src/app/router.tsx`、`src/web/src/features/canvas-plugins`、`src/services/platform-control/src/catalog_api.rs` 及新管理模块、`src/services/platform-control/src/work_packages.rs`、`src/services/agentx-v2-runtime/src/worker_runtime.rs`、新 `plugin_*` 模块、`deploy/docker/backend.Dockerfile`。

门禁：E2E-01/02/03/04 的最小路径通过，Artifact 在 Runtime 域可用，浏览器控制台无 React/模块/CSS 错误，运行输出能被下游引用。

## 4. P6-02：管理、版本与制品生命周期

- [x] P6-02-01 完整列表/详情/版本/使用情况/开发说明页签，中英文本地化、浅深主题和标准状态。
- [x] P6-02-02 上传状态查询、取消、过期、失败重试、确认幂等和同版本摘要冲突。
- [x] P6-02-03 实现启停、默认版本指针、显式版本选择；Catalog 刷新不替换已有节点实例。
- [x] P6-02-04 实现整包版本切换预览，统一处理受影响节点、端口/边/引用、配置错误和 Undo；不写 migration 函数。
- [x] P6-02-05 Draft/Revision/Version/Deployment 包引用入账，乐观锁、删除竞态和服务端权限检查。
- [x] P6-02-06 实现引用保护的版本删除与卸载、包下载、失败对象补偿及上传 TTL 清理。
- [x] P6-02-07 生产、Draft Debug、子 Workflow、Fork、评测全部使用同一依赖闭包，不能遗漏隐藏执行入口。
- [x] P6-02-08 制品校验、冷缓存、同摘要重新下载、缺包失败、Worker API/capability 匹配与激活门禁。
- [x] P6-02-09 历史 UI 通过执行授权获取冻结制品，建立保留/GC 引用；停用不影响现有 Deployment 和恢复。
- [x] P6-02-10 审计与权限种子，普通设计者能使用节点但不能越权管理包或读取其他 Workflow 详情。

门禁：E2E-05/06/07/08/09/10/11/12/13/14/15；两版本并存于不同 Workflow，更新/停用/删除的产品说明与真实执行完全一致。

## 5. P6-03：UI、设计时操作与内置包

- [x] P6-03-01 提取公共 NodePanelContext、编辑事务、SmartInput、资源选择、只读结果和 Trace Loader 出口。
- [x] P6-03-02 固定 React/JSX runtime 注入与私有依赖打包，预编译 CSS、资产 URL、i18n/主题/Portal 与 ErrorBoundary。
- [x] P6-03-03 Runtime 设计时 operation 使用短期 Service JWT、独立有界容量、deadline 与进程回收；不占用业务 Worker Claim/Lease，Control 不启动 Node。
- [x] P6-03-04 实现 resolveDefinition 的纯 JSON 输入、完整性状态、权威 Schema/端口输出及 Hash 缓存。
- [x] P6-03-05 实现动态 Provider 的资源快照、搜索/分页、超时取消与响应过期检查；可调用已有凭据测试/字段发现能力。
- [x] P6-03-06 将契约解析接入草稿保存、发布预览、Compiler/IR；保留不完整草稿可保存及非法引用拒绝规则。
- [x] P6-03-07 Set 完整迁入 agentx/data，用公开 SDK 保持输入转换、输出 Schema、Lineage 与错误一致；删旧执行与推导分支。
- [x] P6-03-08 List 完整迁入 agentx/data，保持 filter/sort/take 次序、稳定输出与逐 Item 语义；删旧执行与推导分支。
- [x] P6-03-09 HTTP 完整迁入 agentx/http，用 ctx.http 复用 Rust Provider/账本/出口/Artifact；删旧节点组装分支，保留宿主能力。
- [x] P6-03-10 将 If/Merge/Loop/Approval/Sub-workflow/Model/Agent/Code 的定义与 UI 纳入 agentx/core，有限原生执行绑定直接调用已有内核。
- [x] P6-03-11 Start/Exit 边界视图共用 UI 注册与包锁；保留边界 Span，不增加伪 Node/Attempt。
- [x] P6-03-12 删除固定 ACTION_PANELS 和 Rust 重复业务 Manifest 构造，重建 Studio Catalog Fixture 与漂移测试。
- [x] P6-03-13 复制粘贴、包缺失、停用、动态端口、版本切换、只读/Undo/断线引用修复完整组件与浏览器测试。
- [x] P6-03-14 检查所有节点 UI 风格、桌面尺寸、键盘操作、性能与挂载资源回收，完成内置矩阵逐项验收。

落点：`src/web/src/features/workflow-designer/{panels,nodes,forms,model,store,api}`、`src/web/src/shared/ui`、`src/crates/agentx-runtime/src/{registry,compiler}.rs`、`src/services/agentx-v2-runtime/src/worker_runtime_builtin.rs`、正式内置包与 SDK。

门禁：E2E-16/17/18/19/20；全内置矩阵均有入口和回归，不能以 Set 成功替代 Loop/Approval/Agent。

## 6. P6-04：运行治理和部署

- [x] P6-04-01 完整双向 JSON-RPC、分帧、requestId/invocationId、握手、方法/版本错误和有界 I/O。
- [x] P6-04-02 进程池总预算、按摘要实例、空闲回收、调用状态清理，设计时与业务并发容量隔离。
- [x] P6-04-03 ctx.http/model/credentials/artifacts 使用冻结资源与现有宿主能力，不复制权限/网络/计量协议。
- [x] P6-04-04 多端口/逐 Item 参数/Schema/Artifact/Lineage 校验；确定性失败不可盲目重试。
- [x] P6-04-05 宿主 RPC 重传去重、逻辑幂等标识、外部写操作的 Outcome Unknown 与结果固化重试。
- [x] P6-04-06 deadline 覆盖装载/等待，取消传播、Lease 丢失停止调用、迟到结果 fencing。
- [x] P6-04-07 Runner 崩溃/死循环/异常 stderr/协议污染/后台计时器场景；强制终止并回收。
- [x] P6-04-08 Linux 进程组与 Windows 本地进程树回收，Worker drain/Pod 退出和无孤儿进程证据。
- [x] P6-04-09 Worker 崩溃、重新派发、冷缓存和 Runtime OSS 故障，恢复不依赖原进程内存。
- [x] P6-04-10 固定 Worker Node/Runner 镜像、Values Schema、capability/Readiness、资源预算、发行和回滚检查。
- [x] P6-04-11 建立冷/热装载、IPC、吞吐、内存与 100/300 节点 UI 基线；确认合理并发下服务不过量启动 Node。

门禁：E2E-21/22/23/24/25/26/27/28/29；分别报告 Windows 开发和 Linux/Kubernetes 结果，不能把一方的运行代替另一方。

## 7. P6-05：Trace 扩展闭环

- [x] P6-05-01 扩展 plugin_operation/plugin_content、包来源、内容版本与 renderer 描述；更新 Schema、初始 DDL 和前端类型。
- [x] P6-05-02 SDK Span/Context 与平台 UUID/eventId/序号映射，开始/更新/结束实时桥接，覆盖 Promise 并发与嵌套。
- [x] P6-05-03 宿主 HTTP/Model 调用挂接插件父 Span，保留 runtimeCallId，避免双重调用或计量。
- [x] P6-05-04 自定义内容 Schema 校验、Preview 预算、Artifact 外置、已有脱敏与执行下载授权。
- [x] P6-05-05 Observability 摄取、Summary/Detail/contents 返回包信息；支持动态内容，不再为每插件加 enum 分支。
- [x] P6-05-06 统一 Trace renderer 注册与只读 Context；Studio/执行详情/节点面板共用；标准数据视图始终可见。
- [x] P6-05-07 历史版本 UI/Schema 与当前插件更新解耦，缺包/组件失败明确提示，不能读取最新版代替。
- [x] P6-05-08 乱序、重复、缺开始/结束、Runner/Worker 崩溃、ClickHouse 延迟/不可用和 Trace 预算耗尽。
- [x] P6-05-09 平台计量/插件自报来源标记、父子成本去重以及节点总耗时与插件子耗时的展示。
- [x] P6-05-10 无埋点插件、多个同类型内容、业务表格 renderer 与嵌套 HTTP 示例自动化。

落点：`src/crates/agentx-runtime-contracts/src/query.rs`、`src/services/agentx-v2-runtime/src/{trace_delivery,trace_artifact,worker_runtime}.rs`、`src/services/observability/src`、`deploy/migrations/observability`、`src/web/src/features/{traces,executions}`。

门禁：E2E-30/31/32/33/34/35；Trace 中断不改变业务权威结果，历史数据不依赖当前安装状态。

## 8. P6-06：开发模板和文档

- [x] P6-06-01 提供独立模板目录、锁文件、SDK tarball、可运行 JSON 映射与 HTTP/Trace 示例。
- [x] P6-06-02 实现 dev/check/test/build/pack:plugin，UI 预览用真实 SDK，运行测试用正式 Runner。
- [x] P6-06-03 编写根 AGENTS.md 与字段级 node-contract/ui-sdk/runtime-sdk/trace/testing 文档，全部引用可本地打开。
- [x] P6-06-04 协议类型、示例 JSON、文档与包版本同源生成/漂移检查，禁止文档示例调用不存在接口。
- [x] P6-06-05 新管理菜单“下载开发模板”、包详情说明与内置样例下载可用。
- [x] P6-06-06 独立目录开发实验：仅凭模板和需求完成变体，检查输出并通过真实页面导入执行。
- [x] P6-06-07 CI 从导出 ZIP 自动验证模板、变体、独立构建、打包与导入校验，模板不依赖内部路径。

门禁：E2E-36/37；记录 AI/人工开发实验的边界，不将开发预览报告写成产品系统验收。

## 9. P6-07：最终收口

- [x] P6-07-01 逐项核对本文与产品/契约/UI/运行/模板文档，修改过的架构事实同步到 docs/01–13 对应章节。
- [x] P6-07-02 执行删除清单，清除旧 Manifest 来源、固定 UI 表、已替换 Set/List/HTTP 分支及旧 Fixture。
- [x] P6-07-03 校验所有后端/前端文件边界、协议漂移、Rust/TS/Python、构建、Helm 渲染与依赖锁定。
- [x] P6-07-04 运行插件聚焦 Kubernetes E2E；定位修复后运行完整 `pytest tests/e2e`，不得用聚焦通过代替全量回归。
- [x] P6-07-05 校验正式内置节点、Application 调用、Agent/Code/Sandbox、Loop/Approval/Sub-workflow 和已有 Trace 核心链路。
- [x] P6-07-06 完成性能与资源证据，持续运行中插件更新/停用/Worker 替换不改变版本和输出。
- [x] P6-07-07 清理临时 namespace/容器/进程与上传对象，恢复测试前开发副本；记录成功和失败路径的 finally 证据。
- [x] P6-07-08 新增最终验收文档，列出提交、镜像、包摘要、命令、耗时、结果、未运行项目和已知限制；按证据更新状态。

## 10. 删除与保留清单

| 处理 | 对象 |
|---|---|
| 删除 | NodeRegistry 固定公开业务节点白名单及重复 Manifest 构造；改由包/能力解析 |
| 删除 | Catalog “仅 Rust Registry 来源”限制及相关过时错误测试 |
| 删除 | ACTION_PANELS、各页面单独维护的插件/内置结果映射 |
| 删除 | 已由 TypeScript 接管的 Set/List 执行与契约推导、HTTP 节点组装分支 |
| 删除 | 过时 Action/Lifecycle 接入文档、旧生成物与旧 Fixture；不恢复旧协议 |
| 保留 | Rust Compiler 拓扑/Binding/Schema 权威、Loop/Approval/Sub-workflow 等核心语义 |
| 保留 | ProviderHttpClient、Vault/Egress、Artifact、runtime_calls、Lease/Fencing/Recovery |
| 保留 | Agent Core、Sandbox Manager、Code 已有执行环境和资源附件 |
| 保留 | Trace Outbox、Relay、ClickHouse 查询、标准结构化展示和权限 |

清理前用真实引用搜索核对责任归属；完成新切片后一次切换，不发布双执行入口。不相关技术债单独记录，不能借插件项目顺便换框架或重建全系统。

## 11. 检查命令与证据约定

最终验证使用以下仓库入口：

```bash
cargo xtask check
pnpm --filter @agentx/web test
pnpm --filter @agentx/web build
uv run --frozen --group test pytest tests/e2e --values deploy/values/local.yaml --scale-down-development
```

聚焦插件入口按 [E2E 文档](06-e2e-acceptance.md) 新增，最终全量仍是同一 pytest 根入口。包构建、格式、单元测试和组件测试按切片完成即跑；已通过后只在新增变更或未解决风险下扩大重跑。

每阶段证据位于 `docs/plan6/evidence/p6-XX.md`；最终报告 `p6-10-final-remediation.md` 引用 `artifacts/e2e/<run-id>/` 的实际产物，并记录源码、镜像与包摘要、测试结果、环境恢复和外部测试边界。
