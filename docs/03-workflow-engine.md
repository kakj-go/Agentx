# Workflow 运行引擎

## 1. 与 n8n 一致的核心语义

平台遵循以下运行方式：

- Workflow 由 Nodes 和 Connections 组成。
- 节点通过输入输出端口连接。
- 节点接收一组 Items，并产生零个或多个 Items。
- 条件节点可以关闭某些输出分支。
- 节点可以在循环中执行多次。
- 手动运行保存每个节点的完整输入输出。
- 发布版本固定节点类型及其版本。
- Trigger 创建一次独立 Execution。
- Wait、审批和外部事件可以挂起并恢复 Execution。

## 2. Workflow Definition

Workflow Definition 包含：

- 标识和名称
- Nodes
- Connections
- Variables
- Settings
- Trigger 配置
- 错误策略
- 默认超时和重试策略

Node Instance 包含：

- Node ID
- 显示名称
- Node Type
- Node Type Version
- 画布位置
- Parameters
- Credential References
- Retry Policy
- Timeout
- Disabled
- Notes

Connection 包含：

- Source Node
- Source Port
- Target Node
- Target Port
- Connection Type

连接类型分为：

- main：普通数据和控制流
- ai_model：Agent 使用的模型
- ai_tool：Agent 可调用的 MCP Tool；资源引用类型固定为 `mcp_tool`
- ai_memory：Agent 使用的 Memory
- ai_retriever：Agent 使用的 RAG
- ai_output_parser：Agent 结果解析器

资源连接声明能力，不等同于主执行顺序。

## 3. Item 数据模型

每个 Item 包含：

- json：结构化数据
- binary：二进制 Artifact 引用
- pairedItem：来源 Item
- metadata：运行期内部信息

节点运行上下文至少包含：

- executionId
- nodeExecutionId
- nodeId
- runIndex
- itemIndex
- branchIndex
- iterationIndex
- workflowVersion
- tenantId
- sessionId
- currentUser

一个节点可采用以下执行模式：

- executeOnce：对整批 Items 执行一次
- executeForEachItem：逐 Item 执行
- executeBatch：按批次执行
- trigger：产生初始 Items
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

Node Execution 的唯一维度不能只有 executionId 和 nodeId，还需要：

- runIndex
- branchIndex
- iterationIndex

Retry 不增加 runIndex，而是增加 Node Attempt。

## 5. 表达式系统

表达式可以读取：

- 当前 Item
- 当前节点的全部输入
- 已执行上游节点的输出
- Workflow 变量
- Execution 信息
- Session 和用户信息
- 循环上下文
- 环境配置
- Credential 的受控字段

服务端不得直接执行任意 JavaScript 表达式。建议：

1. 定义受限表达式语法。
2. 解析成 AST。
3. 校验允许访问的对象和函数。
4. 在受限解释器中执行。
5. 对 Secret 输出做禁止或脱敏处理。

表达式解析错误属于节点配置错误，应明确区分于节点业务错误。

## 6. Node Definition

Node Definition 包含：

- 类型和版本
- 分类和图标
- 参数 JSON Schema
- UI Schema
- 输入输出端口
- Credential 要求
- 执行模式
- Runner 类型
- 默认超时
- 默认重试
- 是否有副作用
- 是否需要沙箱
- 是否支持测试 Mock

Runner 类型：

- Native Rust
- HTTP Connector
- Remote Connector
- Sandbox Python
- Sandbox JavaScript
- Sandbox Shell

发布后固定 Node Version。节点升级通过迁移器修改草稿，不能原地改变历史 Workflow Version。

## 7. Workflow 编译

Draft 保存为 Version 前编译为内部 IR。

编译过程检查：

- Node Type 和版本是否存在
- 必填参数
- 端口类型
- 不可达节点
- 没有 Trigger
- 非法图环
- Merge 等待条件
- 表达式引用
- Credential 是否存在
- Model、MCP Tool、Skill、RAG、Memory 和独立 Credential 授权
- Sub-workflow 版本
- 副作用节点配置
- 运行预算

任意循环只能通过显式 Loop 节点表达，避免一般图环导致调度状态不可判定。

IR 应预先计算：

- 入边和出边
- 节点依赖
- 分支关闭传播规则
- Join 策略
- Agent 资源依赖
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

Node Execution 状态：

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

连接状态：

- Pending
- Produced
- ClosedWithoutData
- Failed
- Cancelled

ClosedWithoutData 非常重要。IF 未选择的分支必须被标记关闭，否则 Merge 会永久等待。

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

Loop 支持：

- 最大迭代次数
- 每批数量
- 并行度
- 终止条件
- 每轮结果合并策略
- 单项失败策略

每一轮都有 iterationIndex，并生成独立 Node Execution。

## 10. Scheduler

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

## 11. 错误和重试

节点错误策略：

- Stop Workflow
- Retry
- Continue
- Emit Error Item
- Error Output
- Trigger Error Workflow

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

Pin Data 只属于 Draft 和手动调试。发布时必须移除或阻止带 Pin Data 的草稿发布。

## 13. 控制面 Definition 与运行协议边界

M2.1 的最小画布只保存 `manual_trigger`、`model`、`mcp_tool`、`skill`、`rag` 和 `memory` 节点。资源节点通过 `resourceReferences` 引用控制面资源；MCP Tool 使用 `resourceType=mcp_tool`，不接受旧 `tool` 类型。

React Flow 的节点和连线状态必须经过 Serializer 生成独立 Workflow Definition。阶段 08 编译器再把 Definition 转换为运行 IR，因此画布数据、控制面 Definition 和运行状态不能共用一个对象模型。
