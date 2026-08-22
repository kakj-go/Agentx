# Workflow Definition 5.0

## 1. 版本边界

Workflow Definition 只接受 `schemaVersion: "5.0"`。本版本是开发期破坏性升级，不读取、迁移或保存旧 Definition；旧字符串表达式直接返回 `LEGACY_EXPRESSION_NOT_SUPPORTED`。

## 2. DynamicValue

所有动态字段持久化为结构化对象：

- `literal`：固定 JSON 值。
- `reference`：单个 `ValueSelector` 与 `missingPolicy`。
- `template`：有序 Text/Reference Segment，适合 Prompt 和 URL。
- `expression`：Visual Builder 生成的递归 AST。

AST 支持 Literal、Reference、Unary、Binary、Conditional、Call、Array 和 Object。二元运算覆盖比较、算术、逻辑和 `in`；函数是固定白名单，`matches` 使用限长的线性时间 Regex。

Runtime 对一次节点激活同时生成 `ResolvedParameters.common` 与按输入端口、Item 顺序展平的 `ResolvedParameters.perItem`。批量级参数消费 `common`；Filter、IF、Switch 和逐 Item 转换必须消费与当前 Item 对齐的 `perItem`，不能用首项解析结果替代整批。Trace 同时记录脱敏、限长后的两类解析结果，Remote Action 按 Node Protocol 原样接收两者。

## 3. Selector

`ValueSelector` 包含 namespace、可选 stable `sourceNodeId`、port、run、item 与类型化 path。可访问命名空间仅为 `inputs`、`outputs`、`contexts`、`execution`、`item` 和 `loop`。Output 引用必须指向拓扑可达的前驱节点；显示名称不参与持久身份。

缺失策略为：

- `error`：确定性失败。
- `null`：产生 JSON null。
- `default`：递归求值默认 DynamicValue。
- `omit`：在对象、Projection、可选 End Output 或 Context Write 中删除/跳过该成员。

## 4. 输出契约

Model 与 Agent 固定返回同一 `AiResponse`：`text/reasoningContent/structuredOutput/files/citations/usage/finishReason/partial`。`usage` 固定包含 `inputTokens/outputTokens/totalTokens/costMicros`。普通变量目录不公开 Provider message、tool calls、Agent iterations 或原始响应；这些诊断信息只进入受权 Trace，原始响应超过 16 KiB 时写入 Artifact 并由 Trace span 引用。

其他节点也必须由 Manifest 声明输出 Schema。工具类返回 `text/structuredOutput/files`，HTTP 返回 `statusCode/headers/body/files`，Code 返回 `stdout/stderr/exitCode/structuredOutput/files/partial`，RAG、Memory、Approval 和 Wait 使用各自稳定语义字段。数据节点继续直接传递 Item 字段，不增加 `data` 或 `result` 包装。所有 Error Port 固定返回 `code/message/retryable/details/sourceNodeId/nodeExecutionId`。

Compiler 将基础 Schema、节点实例配置和 Projection 合成为 `EffectiveOutputContractV1` 并冻结到每个 `CompiledNodeV1`；Worker 成功提交前必须逐端口验证，失败统一为 `NODE_OUTPUT_SCHEMA_VALIDATION_FAILED`。

当目标 Schema 为 `string` 时，Compiler 在 IR 的 Reference 上冻结 `coerce: string`。字符串原样保留，其他 JSON 使用对象键排序、无额外空白的规范 JSON 文本；Studio 必须显式展示“自动转为文本”。未知类型只能转入字符串目标，不能进入其他强类型目标。Template 天然执行同一文本转换。

Runtime 为参数、Projection、Context Write 和 End 的每次文本转换写入受权 Trace 诊断记录，包含目标路径、Selector、来源 JSON 类型、转换模式和结果字节数，不保存来源值或转换后的正文。这样可以定位“为什么可被字符串位置引用”，同时不复制业务数据或 Secret；End 转换使用独立的 `end.string_converted` span。

## 5. 失败与恢复

引用缺失、动态值运算、Projection、Context Write、节点输出和 End Schema 失败属于确定性错误：Attempt/Node/Execution 进入不可重试终态，Lease 被释放，Worker Result 被接收并 ACK。基础设施错误回滚事务并交给 Recovery。

每个 Attempt 有绝对 `deadline_at`。过期任务不再 Claim；Recovery 将 Execution 标记 `timed_out`，Attempt/Node 标记 `timed_out`，释放 Worker Lease 并完成终态物化，随后不会重新排队。查询详情从 Attempt 或最后一个 Worker Lease 返回真实 `workerId`。

Worker 在执行、结果固化和结果提交重试阶段统一续租。心跳仅对瞬态数据库不可用执行 50/100/150ms 的有界重试；Lease 冲突和 fencing 失败立即停止，避免瞬态 MySQL deadlock 丢弃已完成结果，也不会让失去所有权的 Worker 继续提交。

Redis Stream 和 Consumer Group 丢失后由 Worker 按 capability 原地重建，并从 `0-0` 继续消费。Redis 发布失败会以 owner 和 fencing token 条件立即释放 MySQL Outbox Lease 后有界重派；重复发布由 Attempt Claim 幂等吸收，不等待 30 秒 Lease 与节点绝对 deadline 竞争。

## 6. Studio

普通输入展示固定值，变量展示 Chip，Template 使用 Lexical Token Editor，Expression 使用递归 Visual Builder。复制粘贴使用 Agentx 自定义 MIME 保留 Selector；界面不展示或生成字符串占位符。JSON 和 Mapper 的每个叶子都可以绑定 DynamicValue。
