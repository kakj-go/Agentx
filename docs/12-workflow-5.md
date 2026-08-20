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

## 3. Selector

`ValueSelector` 包含 namespace、可选 stable `sourceNodeId`、port、run、item 与类型化 path。可访问命名空间仅为 `inputs`、`outputs`、`contexts`、`execution`、`item` 和 `loop`。Output 引用必须指向拓扑可达的前驱节点；显示名称不参与持久身份。

缺失策略为：

- `error`：确定性失败。
- `null`：产生 JSON null。
- `default`：递归求值默认 DynamicValue。
- `omit`：在对象、Projection、可选 End Output 或 Context Write 中删除/跳过该成员。

## 4. 输出契约

Model 固定返回 `text/message/reasoningContent/structuredOutput/citations/toolCalls/files/usage/finishReason/partial`。其他节点也必须由 Manifest 声明输出 Schema。Compiler 将基础 Schema、节点实例配置和 Projection 合成为 `EffectiveOutputContractV1` 并冻结到每个 `CompiledNodeV1`；Worker 只按此 IR 契约验证结果。

## 5. 失败与恢复

引用缺失、动态值运算、Projection、Context Write、节点输出和 End Schema 失败属于确定性错误：Attempt/Node/Execution 进入不可重试终态，Lease 被释放，Worker Result 被接收并 ACK。基础设施错误回滚事务并交给 Recovery。

每个 Attempt 有绝对 `deadline_at`。过期任务不再 Claim；Recovery 将 Execution 标记 `timed_out`，Attempt/Node 标记 `timed_out`，释放 Worker Lease 并完成终态物化，随后不会重新排队。查询详情从 Attempt 或最后一个 Worker Lease 返回真实 `workerId`。

Worker 在执行、结果固化和结果提交重试阶段统一续租。心跳仅对瞬态数据库不可用执行 50/100/150ms 的有界重试；Lease 冲突和 fencing 失败立即停止，避免瞬态 MySQL deadlock 丢弃已完成结果，也不会让失去所有权的 Worker 继续提交。

Redis Stream 和 Consumer Group 丢失后由 Worker 按 capability 原地重建，并从 `0-0` 继续消费。Redis 发布失败会以 owner 和 fencing token 条件立即释放 MySQL Outbox Lease 后有界重派；重复发布由 Attempt Claim 幂等吸收，不等待 30 秒 Lease 与节点绝对 deadline 竞争。

## 6. Studio

普通输入展示固定值，变量展示 Chip，Template 使用 Lexical Token Editor，Expression 使用递归 Visual Builder。复制粘贴使用 Agentx 自定义 MIME 保留 Selector；界面不展示或生成字符串占位符。JSON 和 Mapper 的每个叶子都可以绑定 DynamicValue。
