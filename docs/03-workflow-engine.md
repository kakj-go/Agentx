# Workflow 运行引擎

## 1. Agentx 原生 Workflow 语义

平台遵循以下运行方式：

- Workflow 由 Nodes 和 Connections 组成。
- 节点通过输入输出端口连接。
- 节点接收一组 Items，并产生零个或多个 Items。
- 条件节点可以关闭某些输出分支。
- 节点可以在循环中执行多次。
- 手动运行保存每个节点的完整输入输出。
- 发布版本固定节点类型及其版本。
- Trigger Binding 把外部事件映射为 Start Inputs，并创建一次独立 Execution。
- Wait、审批和外部事件可以挂起并恢复 Execution。

这些是 Agentx 自己冻结的运行语义。Studio 可以采用 n8n 式拖拽、配置和调试交互，但不以 n8n Workflow JSON、表达式、npm 社区节点或插件协议作为兼容目标。

## 2. Workflow Definition

当前运行定义为不兼容旧版本的 `WorkflowDefinition 7.0`。项目尚未发布，不保留旧版本双读或迁移；开发数据、Fixture、Schema 和编译测试一次性切换。完整契约见 [Workflow 5.0](12-workflow-5.md)。

Workflow Definition 包含：

- 固定虚拟 Start，以及不可变 Inputs Schema 和 Context Contract
- Nodes（含多个 `exit` 结束节点：终止点表达为真实节点，`__end__` 仅保留为编译期虚拟锚点，画布不再渲染）
- Connections
- 唯一正式 Outputs Contract（字段契约存于 `end.outputs`/`end.error.outputs`，全局共享；每个 exit 节点在自己的 parameters 中维护 main/error 两组取值映射）
- Settings
- 错误策略
- 默认超时和重试策略
- Execution Order
- 最大节点激活次数和循环预算

Node Instance 包含：

- Node ID
- Workflow 内唯一且发布后稳定的 Node Key
- 显示名称
- Node Type
- Node Type Version
- Parameters
- Credential References
- Retry Policy
- Timeout
- Execute Once
- Always Output Data
- On Error
- Disabled

Connection 包含：

- Source Node
- Source Port
- Target Node
- Target Port
- Connection Type
- 同一 Source Port 下的稳定 Order

连接类型分为：

- main：普通数据和控制流
- ai_model：Agent 使用的模型
- ai_tool：Agent 可调用的 MCP Tool；资源引用类型固定为 `mcp_tool`
- ai_memory：Agent 使用的 Memory
- ai_retriever：Agent 使用的 RAG
- ai_output_parser：Agent 结果解析器

资源连接声明能力，不等同于主执行顺序。

纯编辑信息保存在独立 Editor Document，包括 Node Position、Viewport、Notes、Group 和折叠状态。Pin/Mock 和临时输入保存在 Debug Overlay；两者都不属于 Workflow Definition，也不能被 Compiler 或 Worker 读取。

## 3. Item 数据模型

每个 Item 包含：

- json：结构化数据
- binary：二进制 Artifact 引用
- pairedItems：零个、一个或多个来源 Item
- metadata：运行期内部信息

每个来源引用至少包含 sourceNodeExecutionId、sourceRunIndex、sourceOutputIndex、sourceItemIndex 和 targetInputIndex。Merge、聚合、笛卡尔积和复杂转换必须能够为一个输出记录多个来源；不能把来源限制为单个可选值。

节点运行上下文至少包含：

- executionId
- nodeExecutionId
- nodeId
- runIndex
- itemIndex
- branchIndex
- loopIterationIndex
- workflowVersion
- tenantId
- sessionId
- currentUser

一个节点可采用以下执行模式：

- executeOnce：对整批 Items 执行一次
- executeForEachItem：逐 Item 执行
- executeBatch：按批次执行
- action root：没有普通入边的业务节点消费 Start 创建的初始 Items
- waitAndResume：持久化等待
- agent：运行模型和工具循环
- subWorkflow：创建子 Execution

## 4. Item 来源关系

节点输出必须记录来源，以便：

- 表达式查询对应上游 Item
- Trace 展示数据来自哪里
- Merge 节点正确合并
- 部分执行时恢复所需数据
- 测试报告定位错误样本

Node Execution 表示节点的一次逻辑激活，稳定身份使用 nodeExecutionId，业务查询维度至少包含：

- runIndex
- 可选的 loopIterationIndex

Retry 不增加 runIndex，而是增加 Node Attempt。

branchIndex/outputIndex 属于 Item 来源和 Edge Delivery，不属于 Node Execution 主身份。同一次节点激活可以同时消费多个输入端口和多个来源分支。普通图环使用通用 runIndex 表示节点再次激活；loopIterationIndex 只作为显式 Loop Over Items 等节点的辅助元数据。

## 5. 表达式系统

Structured Value 1.0 只能读取以下命名空间：

- `inputs`：Start 校验后的不可变输入。
- `outputs`：按 Node Key、端口和显式 Item/run 选择器读取可达前置节点输出。
- `contexts`：声明的 Execution 或 Session Context。
- `execution`：受控 Execution 元数据。
- `item`：当前 Item。
- `loop`：显式循环上下文。

`execution` 由 Runtime 在 Execution 创建时固化为 `ExecutionContextSnapshotV1`，统一包含执行 ID/开始时间/父执行、Workflow 名称与版本及所属部门、触发来源、发起人及多角色分配、应用、调用和会话。节点求值时只附加当前节点 ID、Node Execution ID、run/item/loop iteration 序号；参数、Output Projection、Context Write、End、Wait/Suspend 必须从同一快照加载，禁止分别拼装字段。Composite 资源携带真实子 Workflow 快照，子执行继承父执行的发起人与调用语义，但将 Workflow 字段切换为固化的子 Workflow ID、名称、版本和所属部门。用户、角色、部门或名称后续变化只影响未来执行，历史执行保持原快照。

角色分配只公开 `id/code/name/dataScope/scopeDepartment`，并派生 IDs、Codes、Names 数组。无人工触发不生成用户、部门和角色字段；名称仅用于展示，稳定判断使用 ID 或角色编码。这里不公开权限集合、Token、Credential、租户内部配置或 Workflow Service Identity。

服务端不得直接执行任意 JavaScript 表达式。建议：

1. 定义受限表达式语法。
2. 解析成 AST。
3. 校验允许访问的对象和函数。
4. 在受限解释器中执行。
5. 对 Secret 输出做禁止或脱敏处理。

表达式解析错误属于节点配置错误，应明确区分于节点业务错误。

Definition 不保存字符串占位符。所有可绑定值使用带 `kind` 的 `DynamicValue`：固定值为 `literal`，单变量为 `reference`，文本与变量混排为 `template.segments`，条件、比较、算术、函数、数组和对象使用 `expression.root` AST。`ValueSelector` 以稳定 Node ID、端口、run/item 选择和结构化路径定位来源；节点改名不改变引用。缺失值必须声明 `error`、`null`、`default` 或 `omit`，其中 `omit` 会真正删除字段而非写入 `null`。

编译器拒绝旧占位符和未声明引用；运行时引用、投影、Context Write 或 End Schema 的确定性错误必须进入不可重试终态并 ACK Worker 消息。数据库和对象存储错误仍回滚并由 Recovery 重试，不能混入业务配置错误。

## 6. Node Definition

Node Definition 表示节点类型，发布后的不可变版本载荷称为 Node Manifest Version。

Node Definition 包含：

- 类型和版本
- 分类和图标
- 参数 JSON Schema
- UI Schema
- 输入输出端口
- Readiness Policy
- Credential 要求
- 执行模式
- Execution Style
- 默认超时
- 默认重试
- 是否有副作用
- 是否需要沙箱
- 是否支持测试 Mock

Readiness Policy 至少表达：

- 任意输入到达即可执行
- 指定输入全部到达后执行
- 至少 N 个输入到达后执行
- 等待全部前驱分支完成，即使某些分支没有数据

Execution Style：

- builtin：平台内置实现
- declarative_http：由声明式路由、请求和响应映射执行常规 REST 集成
- remote_action：通过版本化 Node Action API 调用外部节点服务

Sandbox Python、JavaScript、Shell 和 Agent 是运行适配或内置节点能力，不要求对外提供语言 SDK。UI Schema 还需要表达条件显示、Collection、Fixed Collection、Resource Locator、Resource Mapper 和动态选项；动态能力通过 load options、list search、resource mapping 和 credential test 等受控 API 提供，不能只依赖 JSON Schema。

发布后固定 Node Version。节点升级通过迁移器修改草稿，不能原地改变历史 Workflow Version。

## 7. Workflow 编译

Draft Revision 调试或保存为 Version 前编译为内部 IR。两种来源都从 Node Catalog 按精确类型/版本加载 Manifest，并将 Manifest Hash 固化到 Execution/Version Snapshot。

编译过程检查：

- Node Type 和版本是否存在
- 必填参数
- 端口类型
- 不可达节点
- Start/End Contract、Node Key 唯一性和旧 Trigger 节点禁用
- 图连接和强连通分量
- 节点 Readiness Policy
- 表达式引用
- Credential 是否存在
- Model、MCP Tool、Skill、RAG、Memory 和独立 Credential 授权
- Sub-workflow 版本
- Context 读写能力、Patch 路径、Schema 和隐藏依赖环
- 副作用节点配置
- 运行预算

main 连接允许连接回已执行节点形成普通图环。Compiler 必须计算强连通分量（SCC），为回边和循环区域生成稳定标识，并把 Workflow 级最大节点激活次数、超时和取消策略编入 IR。显式 Loop Over Items 是批处理工具，不是表达循环的唯一方式。Sub-workflow 版本依赖仍禁止形成递归依赖环，除非未来另行定义受控递归协议。

IR 应预先计算：

- 入边和出边
- 节点依赖
- SCC、回边和激活边界
- 默认分支执行顺序
- Readiness Policy
- 分支关闭传播规则
- Join 策略
- Agent 资源依赖
- 每个节点实例按端口冻结的 Effective Output Contract；它由 Manifest Schema、实例化结构化输出配置和 Output Projection 合成，Worker 提交结果时直接校验该契约
- Error Branch
- 可用的 Checkpoint 边界

## 8. Execution 状态机

Execution 状态：

- Created
- Queued
- Running
- Waiting
- WaitingApproval
- Suspended
- Succeeded
- Failed
- Cancelled
- TimedOut

Node Execution/Activation 状态：

- Pending
- Ready
- Queued
- Running
- Waiting
- Succeeded
- Failed
- Skipped
- Cancelled
- TimedOut

Edge Delivery 状态：

- Pending
- Produced
- ClosedWithoutData
- Failed
- Cancelled

ClosedWithoutData 非常重要。IF 未选择的分支必须被标记关闭，否则 Merge 会永久等待。

Edge Delivery 是某个 source node activation 向目标 input 产生的一次追加式交付，不是 Workflow 生命周期内整条 Edge 的永久状态。其唯一维度至少包含 executionId、edgeId、sourceNodeExecutionId、targetInputIndex 和 deliverySequence。循环中的同一条 Edge 可以产生多次 Delivery。

## 9. 分支、Merge 和 Loop

IF 和 Switch：

- 按条件向一个或多个输出端口发送 Items
- 未命中端口产生 ClosedWithoutData
- 支持逐 Item 判断和整批判断

Merge 支持：

- Wait All
- Wait Any
- Append
- Merge By Position
- Merge By Key
- Cartesian Product
- Select Input

循环支持两类表达：

- 普通图环：通过回边和 IF/Switch 等条件终止
- Loop Over Items：提供批量拆分、逐批输出和 done 输出

循环保护支持：

- 最大迭代次数
- 每批数量
- 并行度
- 终止条件
- 每轮结果合并策略
- 单项失败策略

节点每次激活生成独立 Node Execution 和单调 runIndex。显式 Loop 节点可以额外记录 loopIterationIndex；普通图环不依赖特定 Loop 节点。

Sub-workflow：

- 调用固定的不可变 Workflow Version
- 创建独立子 Execution，并记录 parentExecutionId 和 callerNodeExecutionId
- 支持等待子 Execution 或异步触发
- 输入遵循子 Workflow 声明的 Schema；同步调用返回子 Workflow 终止输出
- Context 使用 Overlay，只有子流程成功后才按字段 Merge Policy 提交
- 父子 Execution 的状态、Trace、成本和取消传播规则必须明确，不把子节点直接展开为父 Execution 的 Node Execution

## 10. Scheduler

Workflow 默认采用 `deterministic` 执行顺序：同一 Source Port 的分支按 Definition 中稳定 Connection Order 推进，不从节点画布坐标推导行为。平台提供显式 `parallel` 扩展，但不能在 `deterministic` 下隐式并行具有可观察副作用的分支。

节点 Ready 判定基于 Node Definition 的 Readiness Policy、当前 run/generation 的输入 Delivery 以及前驱分支完成状态，不能只对 Merge 编写特殊逻辑。

Scheduler 的基本流程：

1. 在 MySQL 中将满足条件的节点变为 Ready。
2. 原子切换为 Queued。
3. 同一事务写入 execution_outbox。
4. Dispatcher 将任务写入 Redis Stream。
5. Worker 消费后尝试取得 Lease。
6. Lease 成功后切换为 Running。
7. Worker 定期写 Heartbeat。
8. 完成后在事务中保存结果并推进下游。
9. Reaper 扫描 Lease 过期任务并按策略重新调度。

Worker 必须容忍：

- 重复消息
- 消息顺序变化
- Worker 突然退出
- Redis 短暂不可用
- Trace 写入延迟

Redis Stream 只承载可重建的派发事实。Worker 的每个 capability 使用独立连接；队列读取必须有界，连接错误或读取超时后主动重建连接，`FLUSHALL`、Consumer Group 丢失或 Redis Pod 替换后由 MySQL Outbox/Recovery 与 `ensure_group` 自恢复。依赖错误重试属于调度器仍在推进，不能把它误报为进程 liveness stalled，也不能依赖 Kubernetes 重启来恢复队列消费。

## 11. 错误和重试

节点错误策略：

- Stop Workflow
- Retry
- Continue
- Emit Error Item
- Error Output
- Trigger Error Workflow

retryOnFail、maxTries、waitBetweenTries、alwaysOutputData、executeOnce 和 onError 都由 Workflow Engine 解释。Node Manifest 可以声明默认值和能力限制，但远程服务不能绕过平台状态机自行调度重试或错误分支。

Retry 配置：

- 最大次数
- 固定或指数退避
- 最大退避
- 可重试错误类型
- 超时是否重试
- 重试前是否需要人工确认

对于有副作用节点，平台不能默认无限重试。

## 12. 手动调试

Studio 支持：

- Run Workflow
- Execute Node
- Execute To Node
- Execute From Node
- Pin Data
- Mock Node
- Stop Execution
- Retry Failed Node
- Fork From Checkpoint

Pin Data、Mock 和临时输入只属于独立 Debug Overlay。发布只读取 Definition 和资源/Manifest 快照，因此 Overlay 可以保留供后续调试，但永远不会进入 Workflow Version 或生产 Execution。

手动调试必须指定一个不可变 Draft Revision。Coordinator 将该 Revision、Manifest、资源、授权和 Debug Plan 固化为 Execution Snapshot 后，再复用正式 Execution Machine；不得创建隐藏 Version，也不得在 Worker 运行期间读取 Draft Head。

## 13. 控制面 Definition 与运行协议边界

资源节点通过 `resourceReferences` 引用控制面资源；MCP Tool 使用 `resourceType=mcp_tool`，不接受旧 `tool` 类型。M6 后可用节点全部来自 Node Catalog，不在前端维护 Node Type 白名单。

React Flow 状态必须经过 Serializer 分别生成 Workflow Definition 和 Editor Document。Compiler 再把 Definition 转换为运行 IR；Debug Overlay 和运行高亮使用第三套独立模型，因此画布、运行定义、调试数据和 Execution State 不能共用一个对象。

阶段 08 固化四类契约：

1. `WorkflowDefinition`：平台内部、可版本化的 Workflow 定义。
2. `NodeManifest`：节点版本、参数/UI、端口、Readiness、执行模式、Credential 和副作用声明。
3. `NodeActionExecution`：普通节点的版本化请求及 `completed | failed | suspended` 结果协议。
4. `NodeLifecycle`：activate、deactivate、poll、webhook、suspend 和 resume 协议。

Node Action 请求至少携带 protocol/node version、executionId、nodeExecutionId、attemptId、runIndex、executionMode、按 connection type/input index 分组的 Items、公共及逐 Item 解析参数、Artifact Reference、Credential Handle、幂等键、Deadline、取消和 Trace Context。结果只能是 `completed(outputs, lineage)`、`failed(code, retryable, details)` 或 `suspended(resumeContract)`；远程节点不能直接修改 Execution、创建 Attempt 或推进下游。

Node API 至少分为 Action Execute、动态 Provider 和 Lifecycle 三组版本化 Endpoint。接入文档必须说明认证、租户/运行身份、协议协商、幂等、超时取消、Artifact、Credential、错误分类、重试责任和 Fixture 验证方式。

首期不发布 Rust、Python 或 JavaScript Node SDK。平台提供 OpenAPI/JSON Schema、认证和幂等规范、接入文档、Fixture 与协议一致性测试；内部 Rust `NodeRunner` 只是 builtin Adapter。外部启动生命周期属于 Trigger Binding 和 Trigger Gateway，不进入 Node Catalog；完整节点配置 UI 使用同一 Node Manifest，不再定义第二套节点描述。

## 14. 节点返回值与可观测边界

运行时统一 Item、Port、Cardinality 和 Error Port 协议，Studio 则只公开 Manifest 中稳定、强类型的语义叶子字段。不得为了表面统一给所有节点增加万能 `result/payload` 对象；Model/Agent 共享 `AiResponse`，数据节点保持 Item 形态，HTTP、Code、Approval、Wait 等按领域输出。

Provider 原始响应、Agent Iteration 和 Tool Call 属于诊断数据，只能通过 Trace/Artifact 查询，不参与普通变量引用。节点成功结果在推进下游前按 IR 冻结的端口 Schema 校验，避免错误直到 End 才暴露。

引用目录按 Manifest 输出树生成，并优先推荐 Model/Agent 的 `text`。对象、数组、数字、布尔值和 null 只有在目标 Schema 为 string 时才允许确定性文本化；该决策冻结到 IR，并在 Trace 中留下不含原值的转换元数据。
