# Rust / Node Runner 执行与动态 Trace

状态：2026-09-13 执行隔离、动态契约、文件桥接与 Trace 降级已完成修复；验证结果及环境限制见 `evidence/p6-11-runtime-boundaries.md`。

## 1. 执行技术

选择 `tokio::process::Command` 启动固定版本 Node.js，加载平台维护的 Runner JS。使用已有 Tokio、Serde、JSON Schema、对象存储和调用账本基础；Worker crate 显式开启所需 `process/io-util` feature，不依赖其他 workspace crate 偶然带来的 feature 合并。

不嵌入 V8/QuickJS/libnode，不增加 Python Runner，也不将普通可信插件送入现有 Code 节点伪装执行。Code 节点原本的 Sandbox 语义保持。

Runner 随 Worker 镜像发行；用户上传的包只有业务 JS 与资产。Node 固定为 `24.20.0`，基础镜像固定为 `node:24.20.0-bookworm-slim@sha256:ba849c60be29959425b8734d57b8b4b7d56f98edd9504c9af091d5281095a71e`，发行与回滚沿用仓库现有清单；安装时不会动态选择 Node 版本。

## 2. 进程模型

- 每个 Worker 按 pluginMaxProcesses 创建独立消费循环，并通过同样大小的 Semaphore 限制 Node 并发。每次调用独占新进程，结束后回收整个进程树；按摘要复用的仅是 Runtime 源码缓存，避免 ESM 模块常驻积累。
- 一个进程同一时刻只运行一个业务或设计时调用；不同调用并发使用不同进程。包升级使用不同摘要目录和实例，不替换正在执行的模块。
- `resolve_definition/provider/execute` 共用 Runner 协议，但设计时流量有独立并发预算，不能挤占全部业务容量。
- 单进程完成后清理调用 Context、订阅、计时器及临时目录；不允许进程内变量承载跨 Attempt 权威状态。
- Worker 成功 Claim 后先检查 deadline 与制品要求，续租贯穿制品准备、Runner 执行、宿主调用与结果提交。
- CPU 同步死循环不能靠 AbortSignal 协作终止；宿主必须能终止所属进程，且不影响其他并发调用。
- 成功、业务失败、协议破坏、超时和取消均结束本次进程生命周期；不保留空闲进程或跨调用模块环境。

## 3. 传输与生命周期

使用 JSON-RPC 2.0，通过 stdin/stdout 传输 UTF-8 单行 JSON，每条消息由换行分帧；JSON 字符串内换行必须转义。禁止把“每次 read 恰好一条消息”作为假设，检查分片、拼包和最大帧预算。

Runner 的 stdout 仅用于协议。`console.*` 重定向到 stderr，并由宿主持续排空、限制容量、附调用身份；原始日志不是业务 Trace 的替代。直接写坏 stdout 属于 `PLUGIN_PROTOCOL_ERROR`。

| 方向 | 方法 | 主要内容 |
|---|---|---|
| Rust → Runner | `runner.initialize` | protocolVersion、sdkApiVersion、Node 版本要求、包目录/摘要 |
| Runner → Rust | initialize response | 实际版本、导出节点、就绪结果；不匹配拒绝 |
| Rust → Runner | `node.execute` | invocationId、包/节点身份、参数、Items、deadline、调用上下文 |
| Rust → Runner | `node.resolveDefinition` | 精确配置及上游契约 Hash，无资源上下文 |
| Rust → Runner | `node.invokeProvider` | provider 名、参数、允许的资源快照、deadline |
| Runner → Rust | `host.http/model/artifact/credential` | invocationId、requestId、结构化参数和父诊断上下文 |
| Runner → Rust | `trace.event` | 业务 Span/事件/内容通知，带本地序号 |
| Rust → Runner | `invocation.cancel` | invocationId、原因与宽限期 |
| Runner → Rust | response | 唯一终态结果或明确 RPC 错误 |
| Rust → Runner | `runner.shutdown` | 停止接受调用并退出 |

JSON-RPC request ID 与业务 invocationId 分开；Rust 和 Runner 使用不同 ID 前缀，双向请求不能互相覆盖。RPC error 表达协议/调用错误，节点业务失败使用结构化 Result，避免两套失败语义混用。禁止用 Notification 承载必须确认的执行结果。

I/O 分别有读任务与有界写队列，避免父子互等死锁。结果、取消、宿主请求响应优先于 Trace；Trace 超预算可丢弃并累计诊断缺失标记，不能堵住节点终态。无法保持协议读写的进程直接终止，交给现有 Attempt 治理处理。

## 4. 执行 Context 与结果

| Context | 语义 |
|---|---|
| `inputs` | 按输入端口组织的 Item 数组，保留 lineage/artifact 引用 |
| `parameters/perItemParameters` | 平台解析并按 Schema 转换后的参数，字段偏移遵循统一 Items 顺序 |
| `execution` | executionId、nodeExecutionId、attemptId、runIndex、iterationIndex、mode 等冻结运行信息 |
| `signal/deadline` | 协作取消和绝对 deadline；宿主仍执行强制终止 |
| `http/model` | 现有 Provider、凭据、出口与调用账本桥接 |
| `credentials` | 仅本次已授权/冻结资源的访问；不允许任意资源 ID 查询 |
| `artifacts` | 读写 ArtifactRef；大文件不通过 stdio Base64 堆积 |
| `trace/logger` | 当前调用上下文内的子 Span、业务内容和诊断日志 |
| `items` | 创建/映射 Item、合并来源等 helper，减少作者手写 Lineage |

正式执行只有两类结果：`completed { outputs }`、`failed { code, message, retryable, details }`。普通插件首期不返回 `suspended`、新的调度任务或任意 Checkpoint。业务 error 端口由现有节点失败路由规则生成，不能让插件绕过终态校验。

输出必须只包含声明端口，Items、基数、JSON Schema、ArtifactRef 和 Lineage 全部经宿主验证；未知端口、错误类型和非法引用属于确定性失败。空输出与恰好一个空对象不同，SDK 类型和测试必须覆盖。

平台公共 context writes、End 构造和 Binding 解析继续由原生层消费；插件不能自己实现另一套表达式或隐式输出投影。

大文件通过同 Worker/Runner 共享的调用级临时目录交换：宿主将已授权 Artifact 流式写入该目录，SDK 返回本次作用域的文件引用；插件输出文件由宿主校验相对路径、大小和 Hash 后上传 OSS。RPC 只携带描述与引用，不携带任意宿主绝对路径。目录随 invocation 清理，文件内容不进入 Trace 或数据库行；正式协议需同时给出小 JSON 数据与文件路径两种明确场景，不能把大文件退回 Base64 帧。

## 5. 外部调用与幂等

- HTTP 内置插件必须用 `ctx.http` 复用现有 ProviderHttpClient、Vault、Egress、runtime_calls 和 Artifact，不通过 Node 原生 fetch 复制业务网络策略。
- 支持第三方 JS SDK，但需要网络的 SDK 必须注入平台 transport 适配。首期不承诺所有 npm 包无需适配即可接入现有出口与诊断。
- 每次宿主调用带稳定 requestId，重传同一请求复用已有账本结果；禁止因为 IPC 重传重复发送外部写请求。
- 区分同 Attempt 内重传与新 Attempt 的业务重试。SDK 明确提供冻结逻辑调用标识供第三方幂等键使用，并说明 Fork/新运行产生新身份；不能把每次 RPC 的随机 ID 当业务去重键。
- 已发送外部写请求但结果未知时，继续按现有 `outcome_unknown` 和恢复规则处理，不能自动标成 retryable 后无限重发。
- Node.js 抛出的异常映射稳定错误码和安全错误栈；Trace 记录代码入口/source map 定位，但凭据沿用已有脱敏。
- Trace Span 重复不形成新的 runtime_call；调用账本与诊断不是同一类实体。

## 6. 超时、取消、崩溃和恢复

1. deadline 覆盖排队、装载、运行与宿主调用，不能装载完再重置预算。
2. 收到取消/Lease 失效后立即停止新的宿主能力请求，向 Runner 发取消并停止结果接受。
3. 有限宽限期后终止 Runner，等待进程回收；`kill_on_drop` 只作为补充，不能用丢弃 Future 代替完整清理。
4. 终止覆盖进程树；Linux 使用进程组与容器 init 配合，Windows 本地开发使用 Job Object 或等价受维护方案。P6-04 做实际测试后冻结实现，不声称单次 child.kill 能杀死所有后代。
5. Worker 退出按现有 drain 停止 Claim，再取消/回收所属 Runner；Pod 退出不能留下长期进程。
6. Runner 意外退出映射 `PLUGIN_PROCESS_EXITED`，没有结果不可视为 completed；迟到或重复 response 经 invocationId、Lease/Fencing 检查拒绝。
7. Worker 整体崩溃由现有 Recovery 恢复 Attempt；新 Worker 从 Runtime 包快照开始，无需原进程/内存/本地缓存。
8. 结果已固化但提交失败时沿用现有提交重试，不重新调用插件业务函数。

## 7. Trace 自动层

保持现有 Execution → Node → Attempt 层级，平台自动生成输入输出、解析参数、开始结束、错误、重试、超时取消和包身份。零埋点插件也能在标准执行页面诊断。

新增通用 `plugin_operation` Span；插件内部 Span 挂在当前 Attempt 或同调用的另一个 plugin_operation 下。平台宿主 HTTP/Model 调用可挂在发起它的插件 Span 下，同时保留 runtimeCallId 和原计量来源。

Scope 由宿主注入。插件不能选择另一个租户/Execution 作为父节点；本地 Span 标识经宿主映射为平台 UUID，并在 started/updated/finished 中稳定复用。接收序号与 eventId 去重，重投不改 occurredAt。

SDK 可复用 OpenTelemetry API/Context 实现异步上下文传播；增加 Agentx bridge/processor 转换实时事件。OTel 64 位 Span ID 与平台 UUID 需要明确映射，不能直接替换平台字段。仅完成时导出的 exporter 不满足运行中展示；started 和中间事件必须在执行中投递。

## 8. Trace 业务内容层

新增固定 `plugin_content` 承载类型；具体业务由以下字段决定：

| 字段 | 用途 |
|---|---|
| `packageId/packageVersion/bundleDigest` | 精确执行来源，也用于历史 UI 解析 |
| `nodeType/typeVersion` | 节点语义身份 |
| `contentType/contentVersion` | 例如 `acme.crm/customer-matches` 与版本 1 |
| `label` | 本次内容的可读标题；展示按节点包本地化能力处理 |
| `preview/contentRef` | 预算内 JSON，或现有执行级 Artifact |
| `rendererKey` | 包中已声明的可选展示组件；从受校验 Manifest 解析 |

一个插件可以产生多个相同类型内容，按 eventId/sequence 展示，不以类型名覆盖前面的输入、过程或输出。业务内容按包声明的 Schema 检查；错误诊断内容记录 warning，不把已成功业务结果改成失败。

需要同时扩展 Trace Envelope、Span Summary、Span Detail、内容 DTO、序列化、ClickHouse 初始 DDL、摄取转换与查询响应。不能只把包 ID 塞 attributes 而让列表/详情无法选择正确 renderer。

现有固定 Span/content 枚举各增加一个正式扩展类型即可；不为每个新插件改 Rust 枚举、ClickHouse 列或前端 switch。

## 9. Trace 展示与计量

- 时间线、父子树、状态、分页、过滤和原始事件属于平台；插件组件只渲染某份只读业务内容。
- `result` 和 `traceRenderers` 共用制品 Loader 与权限规则；Studio、执行详情、Node Inspector 使用同一扩展解析服务。
- 标准结构化视图始终存在；可与插件视图切换。缺少可选 renderer 仍能查看数据；声明的 renderer 加载失败显示错误并保留标准查看入口，不执行其他版本组件。
- 插件移出新建目录不会破坏历史 Trace。若制品被异常丢失，标准数据继续可看，专用视图明确提示丢失；不从当前 Catalog 猜版本。
- 计量由唯一调用实体归属：平台 Model/HTTP 调用使用原账本；插件报告的 usage 标记 `plugin_reported`。聚合不把父 Span、子 Span和业务内容的同一笔成本重复相加。
- 第三方 SDK 未埋点的内部过程无法自动推断，UI 不生成虚构子步骤；节点总耗时仍由平台记录。

## 10. Trace 可靠性

- 使用当前 TraceDraft/Outbox/Relay/Observability 管线，插件不直接连 MySQL、Redis、ClickHouse。
- Preview 预算、递归脱敏、大内容 Artifact、执行级下载授权沿用现有规则。
- Runner 崩溃时 Worker 为已知活动插件 Span 发终止事件；Worker 同时崩溃且无法确认时，展示诊断不完整，不伪造完成时间。
- 业务 Attempt 终态与 Trace 完整性分开；恢复不能依赖插件 Trace。已有 Trace 入队保存点降级继续生效。
- 无界 Span 数量、深度、内容和事件频率必须有统一预算；预算限制属于运行配置，不要求插件使用者填写。
- 乱序/重复、缺开始/缺结束、ClickHouse 延迟/不可用、Artifact 不可用都进入 P6-05/P6-07 验收。

## 11. 部署落点

调整 `deploy/docker/backend.Dockerfile` 的 Worker 镜像目标及 Rust `agentxctl` 的构建/发行路径，为 Worker 提供固定 Node 与 Runner。Control/Observability 不因此启动 Node 进程。

更新 `deploy/helm/agentx-runtime/templates/deployment-workflow-worker.yaml`、Values Schema 与探针/资源预算：进程并发、单进程内存、制品缓存和 drain 时间。只增加满足本期运行的必要运维字段，UI 不展示这些内部参数。

Readiness 检查 Runner 可启动、SDK/API 兼容和必需内置包；不能扫描并启动所有租户插件作为每次健康检查。原生节点运行和缺失插件能力要可区分，发布/派发阶段明确检查所需 capability。

冷启动、缓存命中、进程重启和内存基线要有证据；不声称进程间调用零开销。多 Item 一次按现有执行风格传入，避免逐 Item 启动 Node 进程。
