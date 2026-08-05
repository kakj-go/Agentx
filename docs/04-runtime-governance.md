# 运行记录、Checkpoint、沙箱与审批

## 1. Trace、Checkpoint 和审计事件

三者职责不同：

| 类型 | 用途 | 主要存储 |
|---|---|---|
| Trace | 观察节点、模型、工具和沙箱过程 | ClickHouse |
| Checkpoint | 恢复或派生执行 | MySQL 元数据加对象存储 |
| 审计事件 | 记录发布、审批、权限和人工操作 | MySQL |

Trace 丢失不应改变 Workflow 结果；Checkpoint 丢失会影响恢复；审计事件用于确认操作责任。因此三类数据不能只放在一张 Trace 表中。

## 2. Trace 层级

推荐层级：

- Application Invocation
  - Workflow Execution
    - Node Execution
      - Node Attempt
      - Model Call
      - Tool Call
      - RAG Retrieval
      - Memory Read/Write
      - Sandbox Execution
      - Approval Wait

Trace Event 记录：

- Trace ID、Span ID、Parent Span ID
- Execution 和 Node Execution
- 开始和结束时间
- 输入输出引用
- 状态和错误
- 模型及提供商
- Token 和成本
- Tool 名称、参数和结果
- RAG Query 和召回结果
- Sandbox ID 和资源用量
- Agent Iteration

## 3. 平台 Trace 页面

不依赖 Prometheus 或 Grafana。平台直接通过内部 API 查询 MySQL 和 ClickHouse。

页面能力：

- 按租户、Workflow、版本、状态、时间和发起者查询
- 在画布上显示运行路径
- 查看节点的每个 runIndex 和可选 loopIterationIndex
- 查看输入输出和 Artifact
- 查看模型请求、回复、Token 和成本
- 查看 Agent 每轮工具调用
- 查看节点错误和重试
- 查看审批等待时间
- 从 Checkpoint 创建 Fork
- 对比两次 Execution

MySQL 保存列表摘要，ClickHouse 保存详细事件，避免每次列表查询扫描 Trace 明细。

## 4. ClickHouse 首期设计

首期使用统一追加表 workflow_trace_events，主要字段：

- tenant_id
- trace_id
- span_id
- parent_span_id
- execution_id
- node_execution_id
- event_type
- event_time
- workflow_id
- workflow_version_id
- node_id
- node_type
- model
- provider
- tool_name
- duration_ms
- input_tokens
- output_tokens
- cost
- status
- error_code
- attributes_json
- content_ref

大段 Prompt、Response、Tool Result 和文件放对象存储，ClickHouse 保存摘要或引用。

Trace Writer 应批量写入并支持重试。如果 ClickHouse 不可用，先将 Trace Event 留在队列或本地缓冲中，不阻塞节点完成事务。

## 5. Checkpoint

建议每个成功 Node Execution 之后形成逻辑 Checkpoint。重要 Wait、Approval 和副作用节点可以在执行前后都创建。

Checkpoint 包含：

- Workflow Version
- Execution Snapshot
- 当前图位置
- 已完成节点集合
- Node Activation Frontier
- Edge Delivery Cursor 和待满足输入集合
- Workflow 变量
- 节点输出引用
- Artifact 引用
- Agent State 引用
- Session State 引用
- 父 Execution 和父 Checkpoint
- State Hash

小型状态可以保存在 MySQL，大型 Items 和二进制内容存对象存储。

## 6. Fork Execution

历史执行不可修改。从某节点重新执行时：

1. 找到该节点执行前的 Checkpoint。
2. 创建新的 Execution。
3. 写入 parent_execution_id 和 fork_checkpoint_id。
4. 复用 Checkpoint 之前的节点输出。
5. 重新执行选中节点。
6. 将所有下游视为未执行。

重新执行模式：

- 使用原 Workflow Version 和参数
- 使用兼容的新 Draft
- 覆盖本次节点输入
- Mock 指定节点
- 跳过指定副作用

LLM 和外部工具本身可能不确定，因此 Checkpoint 保证恢复输入状态，不保证重新执行产生相同输出。

## 7. 副作用节点

节点定义必须标明副作用级别：

- None：纯计算
- Idempotent：提供稳定幂等键
- Reversible：存在补偿操作
- Irreversible：不可自动撤销

重新执行 Irreversible 节点前，需要：

- 明确确认
- 或复用旧输出
- 或进入 Dry Run
- 或执行预定义补偿

## 8. Agent 运行

Agent 节点内部循环：

1. 加载消息、Memory、RAG 和上下文。
2. 调用模型。
3. 解析模型 Tool Call。
4. 检查 Tool 是否授权给当前 Workflow。
5. 执行 Tool。
6. 将结果追加到 Agent State。
7. 再次调用模型，直到输出结果或达到限制。

每轮记录：

- iteration
- model request
- model response
- tool request
- tool response
- tokens
- cost
- stop reason
- error

## 9. Agent 循环控制

Workflow 或 Agent 节点可以配置：

- 最大迭代次数
- 最大模型调用次数
- 最大工具调用次数
- 最大 Token
- 最大成本
- 最大持续时间
- 单工具最大错误次数
- 相同工具参数最大重复次数

工具调用指纹由 Tool 名称和标准化参数生成。平台检测：

- 连续相同调用
- A-B-A-B 周期
- 参数只有无意义变化
- 持续返回同一个错误
- 多轮调用后 Agent State 没有实质变化

命中策略：

- 终止节点
- 进入 Error Output
- 暂停等待人工确认
- 返回部分结果和警告

## 10. OpenSandbox

进入 OpenSandbox 的节点：

- Python
- JavaScript
- Shell
- 文件处理
- 浏览器自动化
- 高风险自定义 Tool
- Agent 动态生成代码

Sandbox Manager 负责：

- 创建、复用和销毁
- 模板及镜像版本
- CPU、内存、磁盘和时间限制
- 输入文件上传
- 输出 Artifact 收集
- 网络访问范围
- 短期 Credential 注入
- stdout、stderr 和 exitCode 收集

Agentx 保留与供应商无关的 `SandboxRuntime` Port，`sandbox-manager` 通过 Rust `OpenSandboxAdapter` 直接调用官方 Lifecycle API 和 execd API。Worker 只调用 Agentx 的稳定内部契约，不加载供应商 SDK 或 DTO，也不持有 OpenSandbox API Key。生产链路不得插入 Go/Python Sidecar 或调用 `osb` CLI；官方 Go SDK/CLI 只作为测试差分基准。

协议和版本边界：

- 构建必须固定 OpenSandbox 源码 Commit、Lifecycle/execd Spec Hash 和 Server/execd/egress/模板镜像版本；升级必须显式更新生成物、Fixture 和兼容矩阵。
- OpenAPI 只生成或校验内部 DTO 与普通 HTTP 端点，不视为完整 SDK；SSE、取消、背压、Sandbox 就绪轮询、Endpoint 解析、重试和错误标准化由可审查的 Rust 手写层实现。
- CI 必须校验 vendored Spec Hash 与生成物一致；运行时发现缺失必需字段、未知的不兼容事件或不受支持组件版本时返回 `SANDBOX_PROTOCOL_UNSUPPORTED`，不得猜测字段或降级为成功。

Endpoint 和凭证边界：

- Lifecycle 返回的 Endpoint 必须使用结构化 URL 解析，限定允许的 scheme、host、port 和配置化 CIDR/域名范围；每次刷新都重新校验，禁用跨 Host 重定向，禁止拼接 URL 绕过检查。
- 只转发固定协议定义的鉴权 Header 白名单。OpenSandbox API Key 只发送到配置的 Lifecycle Origin；execd Token 只发送到已校验的 Sandbox Endpoint，不能进入重定向、日志、Trace、Artifact 或用户可见错误。
- Endpoint、Header 或 Token 校验失败必须在网络请求前终止，并记录脱敏的审计原因。

SSE、取消和重试边界：

- SSE 解析必须支持分片和多行 `data`，限制单事件、累计输出和缓冲区大小，并设置连接、空闲和总超时；消费端使用有界队列形成背压，大输出转为 Artifact。
- 取消先停止读取流，再调用 execd interrupt；无论 interrupt 是否成功都进入 Sandbox 终止/回收流程，部分 stdout/stderr 只能标记为部分结果。
- 自动重试仅允许 health/get、Endpoint 解析等只读请求，以及契约明确幂等的 interrupt/terminate；create、command、upload 等操作只有携带并验证服务端支持的幂等键时才能重试。无法判断请求是否已提交时必须返回明确的不确定错误并由 Reaper 对账。

安全基线：

- 创建请求默认 `networkPolicy.defaultAction=deny`，只允许节点声明且经 Workflow Grant 校验的域名或 CIDR。
- 基础镜像固定 digest，默认只读；临时写入只进入受限工作目录，输出通过 Artifact 收集。
- CPU、内存、进程数、磁盘、TTL 和租户并发在 Agentx 与 OpenSandbox 两侧同时限制。
- Credential 优先通过 OpenSandbox Credential Vault 或 Agentx 短期凭证代理注入，不得进入命令行、stdout、stderr、Trace 或持久镜像。
- Sandbox 完成、超时、取消、Worker 失联或 Lease 过期时都必须幂等终止；回收失败进入 Reaper，不得把节点标记为虚假成功。

运行环境边界：

- 本地开发和 CI 可使用 OpenSandbox Docker Runtime + runc，用于功能、资源限制和网络策略 E2E。
- runc 不能作为生产级多租户强隔离结论；生产必须使用 OpenSandbox Kubernetes Runtime，并配置 gVisor、Kata 或经安全评审的等价 RuntimeClass。
- 生产门禁必须检查实际 Pod RuntimeClass、网络策略、资源限制和残留 Sandbox，不能只检查 OpenSandbox `/health`。

支持两类生命周期：

- Node Sandbox：单次节点执行，用完释放。
- Session Sandbox：同一 Session 按 TTL 复用。

Worker 只接收标准化结果，不直接依赖 OpenSandbox 内部数据结构。

## 11. 审批节点

Approval Node 是 waitAndResume 节点。

进入审批时：

1. 创建审批前 Checkpoint。
2. 创建 Approval Task。
3. Node Execution 进入 Waiting。
4. Workflow Execution 进入 WaitingApproval。
5. Worker 释放资源。
6. 创建站内通知。

审批动作：

- Approve
- Reject
- Provide Input
- Cancel
- Timeout

审批完成后：

1. 保存审批动作和表单数据。
2. 将审批结果转换为标准 Item。
3. 按 Approved、Rejected 或 TimedOut 端口继续。
4. 将 Execution 重新加入调度队列。

待办中心只解决 Workflow 审批，不扩展成通用 OA。
