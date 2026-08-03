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
- mcp:view
- mcp:manage
- mcp:discover
- mcp:debug
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
- 哪些 Workflow 可以使用某 MCP Server
- 哪些 Workflow 可以调用某个自动发现的 MCP Tool
- 哪些 Workflow 可以加载某 Skill
- 哪些 Workflow 可以查询某 RAG Collection
- 哪些 Workflow 可以读写某 Memory Namespace
- 哪些 Workflow 可以使用某 Credential

授权控制面使用统一“资源授权”列表聚合 Credential、Model、MCP Server、MCP Tool、Skill、RAG 和 Memory。资源详情页只负责配置、版本和连接诊断，不再各自维护授权面板。统一列表可按资源类型筛选，并分别向 Department 或 Workflow Service Identity 授予 `view/use/read/write/manage`。

发布时进行完整检查，运行时再次校验。资源授权被撤销后，新执行不能继续使用该资源。MCP Tool 的依赖链固定为 `MCP Tool → MCP Server → Credential`，三类资源必须逐项授权，任一 Grant 都不能隐式传递给下一项。

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

Provider 名称、Endpoint、Credential 和状态可通过乐观锁编辑。Alias 名称和状态可直接修改；Model Name、Endpoint Override、Credential 或默认参数变化时创建新的不可变 Deployment Revision，并在同一事务切换 Alias。历史 Revision、价格和切换操作者始终可查询，不能被编辑覆盖。

新建 Model Alias 的连接状态固定为 `untested`。只有用户手动发起 Alias 级连接测试后，状态才更新为 `healthy` 或 `unhealthy`；Provider、Alias 或当前 Deployment Revision 的有效配置发生变化后，旧健康结果立即失效并重新显示 `untested`。

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

## 4. MCP Server 与 Tool

平台不提供人工创建 Tool 的入口。用户接入 MCP Server 后，控制面通过 `initialize` 和分页 `tools/list` 自动发现 MCP Tool；同名且 Schema Hash 相同的结果幂等复用，Schema 变化产生不可变 Tool Version，未再次发现的 Tool 标记为 unavailable。

M2.1 支持 Streamable HTTP 和 Legacy SSE，不支持 stdio。MCP Server 固化传输、Endpoint、Credential Reference 和非敏感配置 Hash；MCP Tool 固化名称、输入输出 Schema、Annotation 和 Schema Hash。用户只能设置 Tool 的启停、调试开关、超时和副作用等级，不能修改服务端声明的 Schema。界面将 JSON Schema 渲染为字段树，直接展示字段名、类型、必填、说明、约束和嵌套结构，原始 JSON 不作为默认阅读界面。

调试调用需要 `mcp:debug`、资源可见性、参数 Schema 校验和危险操作确认。审计只保存参数 Hash、状态和耗时，不保存请求参数、响应正文或 Secret。Workflow Service Identity 必须分别获得 MCP Server、MCP Tool 和 Credential 的 Grant；Server 可见性不能替代 Tool 使用权限。

Sandbox 与 MCP 完全分离。Python、JavaScript、Shell 和其他动态代码由阶段 10 的 CubeSandbox Profile 和 Adapter 处理，不是 MCP Server 或 Tool Runner 类型。

## 5. Skill 管理

Skill 是 Agent 可加载的版本化能力资源，用于封装稳定的指令、资产引用和依赖声明，而不是绕过平台执行权限的新节点。

Skill Definition 描述业务身份和生命周期；新建时自动创建不可删除的根文件 `SKILL.md`。根文件必须包含 `name` 和 `description` frontmatter，前端将 Skill 描述作为正文上方的独立必填输入展示，后端统一生成 frontmatter 并与 Skill 元数据同步。用户可在线创建目录和 Markdown 文件、移动或重命名条目，并把图片、PDF、文本、代码或其他二进制拖拽上传到目录。只有 Markdown 可在线编辑；其他文件只读预览或显示元数据。

Markdown 使用开源 MDXEditor/Lexical 富文本界面编辑，用户通过标题、加粗、列表、链接和表格等可视化控件生成标准 Markdown，不需要掌握 Markdown 语法。编辑器可从当前 Skill 文件树选择目标并插入标准相对链接。移动或重命名文件时服务端同步重写内部相对链接；删除被引用文件会被拒绝。发布前通过 Markdown AST 校验引用，Skill Version 固化每个文件的路径、Artifact、Content Hash、引用目标 Hash，以及 Model、MCP Tool、Credential、Skill、RAG 和 Memory 依赖。ZIP 只用于当前工作区导入导出，不是 Skill 的持久化模型。

Workflow Draft 可以选择 Skill Definition 和目标版本；发布时必须固化 Skill Version，并检查所有直接和递归依赖对 Workflow Service Identity 均已授权。Skill Grant 不能隐式授予其 MCP Tool、Model 或 Credential 权限，任一依赖授权被撤销后，新执行不得继续加载该 Skill。

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
