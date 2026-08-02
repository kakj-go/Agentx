# Workflow 上下游业务模块

## 1. 单公司、部门和权限

首期一个部署只支持一个公司，不提供 Tenant CRUD 或切换入口。所有业务实体仍包含 tenant_id，作为隔离、索引和后续扩展边界。

组织模型：

- Tenant
- Department
- User
- Role
- Permission
- Workflow Service Identity

权限分为两层。

### 操作权限

示例：

- workflow:create
- workflow:edit
- workflow:run
- workflow:publish
- workflow:view_execution
- workflow:fork_execution
- workflow:manage_permission
- model:manage
- tool:manage
- skill:manage
- dataset:manage
- approval:handle
- application:invoke

### 数据范围

- 本人
- 本部门
- 本部门及子部门
- 全租户
- 指定 Workflow
- 指定资源

设计者的权限只决定他能否配置 Workflow。生产运行时使用 Workflow Service Identity，避免 Workflow 继承创建者个人权限。

Bootstrap Admin 是 Company Admin，可以管理全部部门、用户和角色。Department Admin 只能管理其授权部门及子部门，不能访问父级或兄弟部门；新用户由服务端设置固定的一次性初始密码 `123456`，首次登录必须修改，正式密码仍执行 12–128 位规则。

## 2. Workflow 资源授权

资源授权需要表达：

- 哪些 Workflow 可以使用某 Model
- 哪些 Workflow 可以调用某 Tool
- 哪些 Workflow 可以加载某 Skill
- 哪些 Workflow 可以查询某 RAG Collection
- 哪些 Workflow 可以读写某 Memory Namespace
- 哪些 Workflow 可以使用某 Credential

发布时进行完整检查，运行时再次校验。资源授权被撤销后，新执行不能继续使用该资源。

## 3. 模型管理

模型对象：

- Model Provider
- Model Deployment
- Model Alias
- Credential Reference
- Price Version
- Default Parameters
- Workflow Grant

Workflow 推荐引用 Model Alias，而不是将真实 Endpoint 和 Credential 写入节点参数。

模型调用记录：

- Provider
- Model
- Alias
- 请求参数
- Token
- 成本
- 延迟
- 错误

价格必须有版本，历史执行按调用发生时的价格快照计算。

## 4. Tool 管理

Tool 来源：

- HTTP
- OpenAPI
- MCP
- 内部 Connector
- CubeSandbox Code

Tool Definition 包含：

- 输入输出 Schema
- Credential
- 超时
- 重试
- 幂等性
- 副作用级别
- Sandbox 策略
- Workflow Grant

Agent 调用 Tool 前必须检查当前 Workflow Service Identity 的授权。

## 5. Skill 管理

Skill 是 Agent 可加载的版本化能力资源，用于封装稳定的指令、资产引用和依赖声明，而不是绕过平台执行权限的新节点。

Skill Definition 描述业务身份和生命周期；每次内容变化生成不可修改的 Skill Version。Skill Version 包含：

- Manifest
- Source，例如平台内置、Git Repository、上传包或 Skill Registry
- Compatible Runtime
- Prompt 和 Asset Reference
- Tool、Model 和 Credential 依赖
- Content Hash
- Artifact Reference
- Workflow Grant

Workflow Draft 可以选择 Skill Definition 和目标版本；发布时必须固化 Skill Version，并检查所有依赖资源对 Workflow Service Identity 均已授权。Skill 的授权不能隐式授予其依赖的 Tool、Model 或 Credential 权限，任一依赖授权被撤销后，新执行不得继续加载该 Skill。

仅包含 Prompt 和静态资产的 Skill 可由 Agent Runtime 直接加载；包含 Python、JavaScript、Shell 或其他不可信代码的能力必须通过 CubeSandbox 执行，并沿用平台的超时、资源配额、网络和凭证注入策略。

## 6. RAG

首期提供 LightRAG Adapter，但平台层使用统一接口：

- query
- retrieve
- insert
- delete
- healthCheck

授权范围：

- Connection
- Workspace
- Knowledge Base 或 Collection
- Read 或 Write
- Department
- Workflow

RAG 节点的 Trace 包含查询、召回结果引用、Score、耗时和错误。

## 7. Memory

首期提供 Mem0 Adapter，统一接口：

- get
- search
- add
- update
- delete

Memory Scope：

- User
- Session
- Application
- Workflow

Workflow 需要明确自己可访问的 Namespace 及读写权限。

## 8. Application

发布后的 Workflow 可以创建 Application。Application 包含：

- 当前 Deployment
- 输入输出 Schema
- Playground 配置
- Session 配置
- API 配置
- API Key
- 版本路由策略

Application 可以：

- 创建会话
- 发送消息
- SSE 流式响应
- 异步调用
- Webhook 回调
- 查询 Execution 状态
- 取消 Execution

## 9. Session 和 Message

一个 Session 包含多轮 Message，每轮用户输入产生一次独立 Workflow Execution：

- Session
  - User Message
  - Workflow Execution
  - Assistant Message

这样每轮对话都有独立 Trace、成本、Checkpoint 和错误记录。

Session 默认固定 Workflow Version，保证长会话行为稳定。Application 可以选择：

- Session 固定版本
- 每轮使用当前 Deployment
- 管理员显式升级 Session 版本

Playground 使用相同的 Application API，不维护另一套运行路径。

## 10. API

建议的核心接口：

- 创建 Application Session
- 查询 Session
- 发送 Message
- 查询 Message 历史
- 查询 Execution
- 取消 Execution
- 从 Checkpoint Fork
- 获取 SSE Stream
- Webhook Trigger

调用请求需要支持：

- API Key
- Idempotency Key
- 输入 Schema 校验
- 超时
- 同步、流式和异步模式

## 11. Dataset

Dataset 包含不可变 Dataset Version。Test Case 包含：

- Input
- Expected Output
- Context
- Tags
- Evaluator Config

测试数据改变后生成新版本，保证历史报告可复现。

## 12. Evaluation

Evaluation Run 将一个 Workflow Version 和一个 Dataset Version 绑定，批量创建 Test Execution。

首期 Evaluator：

- Exact Match
- Contains
- Regex
- JSON Schema
- 自定义代码
- LLM Judge
- Tool 是否正确调用
- Tool 参数是否正确
- 是否出现 Agent 循环
- 节点错误次数
- 总成本
- 总耗时

报告内容：

- 总通过率
- 每个 Case 的结果
- Workflow Version 对比
- 失败节点分布
- Tool 错误分布
- 成本和耗时分布
- 失败 Case 的 Trace 链接

## 13. 消息中心

消息只围绕 Workflow 业务：

- 待审批
- 审批完成
- Execution 失败
- 测试集运行完成
- Workflow 发布成功或失败
- Agent 达到成本或循环限制

消息可以跳转到 Approval、Execution、Workflow 或 Evaluation Report。
