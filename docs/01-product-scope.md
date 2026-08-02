# 产品定位与范围

## 1. 产品定位

Agentx 是企业级 Workflow 开发、运行、发布和评测平台。

它在开发体验上接近 n8n：

- 拖拽节点和连接线构建 Workflow
- 手动运行整个 Workflow
- 单独执行某个节点
- 查看每个节点的输入和输出
- 使用历史或固定数据继续调试
- 使用 Trigger、Action、Transform、Control 和 Sub-workflow 节点
- 将草稿发布为生产版本

在此基础上增加 Agent 场景需要的能力：

- LLM、Agent、Tool、RAG、Memory 节点和可版本化 Skill
- Agent 内部工具循环记录和限制
- CubeSandbox 中的不可信代码执行
- 节点级 Trace、Checkpoint 和重新执行
- Workflow 中的人工审批
- 测试集、批量评测和版本对比
- 发布后的应用、会话、消息和调用 API
- 围绕 Workflow 资源的部门、角色和数据权限

## 2. 产品主链路

### 开发阶段

1. 创建 Workflow 草稿。
2. 从节点面板拖入节点。
3. 配置模型、工具、Skill、RAG、Memory 或外部连接。
4. 使用表达式映射上下游数据。
5. 手动运行或执行到指定节点。
6. 查看画布高亮、节点输入输出和 Trace。
7. 使用 Pin Data、Checkpoint 或 Mock 数据重复调试。

### 验证阶段

1. 将草稿保存为不可变 Workflow Version。
2. 选择 Dataset Version。
3. 批量创建测试执行。
4. 运行规则评分、代码评分或 LLM Judge。
5. 查看通过率、成本、耗时、工具错误和失败 Trace。
6. 与上一个 Workflow Version 比较。

### 发布阶段

1. 将 Workflow Version 发布为 Deployment。
2. Deployment 绑定到 Application。
3. Application 提供 Playground、会话和 API。
4. 外部请求创建 Session、Message 和 Workflow Execution。
5. 每轮消息都可以查询独立的 Trace 和成本。

### 运行阶段

1. Trigger Gateway 接收 API、Webhook、定时或事件请求。
2. Coordinator 创建 Execution 和版本快照。
3. Scheduler 计算可运行节点。
4. Worker 执行节点并持久化结果。
5. 遇到审批或等待节点时挂起，收到事件后恢复。
6. 遇到故障时按节点策略重试、继续或进入错误分支。

## 3. 核心对象

| 对象 | 含义 |
|---|---|
| Workflow | 工作流的业务容器 |
| Workflow Draft | 当前可编辑内容 |
| Draft Revision | 草稿自动保存形成的历史修订 |
| Workflow Version | 不可修改的发布候选快照 |
| Deployment | 某环境正在运行的 Workflow Version |
| Application | 对外提供 Playground、会话和 API 的应用 |
| Execution | 一次完整 Workflow 运行 |
| Node Execution | 某节点在某次循环或分支中的运行 |
| Node Attempt | 节点一次实际尝试，重试会产生多个 Attempt |
| Trace Event | 模型、工具、节点和沙箱的过程记录 |
| Checkpoint | 可用于继续或派生执行的状态点 |
| Skill | Agent 可加载的版本化能力包及其依赖声明 |
| Dataset | 测试用例集合 |
| Evaluation Run | 某版本针对某测试集的批量评测 |

## 4. 首期产品边界

首期必须完成：

- 企业初始化、部门、用户、角色
- Workflow 画布和节点配置
- Item 数据传递和表达式
- 手动、API、Webhook 和定时触发
- IF、Switch、Merge、Loop、Wait 和 Sub-workflow
- LLM、Agent、Tool、Code、RAG、Memory 和审批节点
- Skill 定义、不可变版本、依赖校验和 Workflow 授权
- Workflow 版本、发布和回滚
- Trace、成本、工具错误和 Agent 循环记录
- Checkpoint 和 Fork Execution
- CubeSandbox 接入
- Application、Session、Message、SSE 和 Playground
- Dataset、Evaluator 和报告

首期一个部署只初始化一个企业，不提供企业创建和切换入口。tenant_id 继续贯穿数据模型，为数据隔离和后续扩展保留稳定边界；企业内通过部门树、角色和数据范围管理权限。

首期不做：

- 通用 OA 和完整 BPMN
- 通用页面搭建
- 大规模 SaaS 连接器市场
- 通用监控平台
- 完整商业计费平台
- 跨区域多活
- 与 n8n 全部社区节点二进制兼容

## 5. 产品原则

- Workflow 优先：新模块必须能够说明它解决 Workflow 哪个阶段的问题。
- 版本不可变：生产执行始终指向明确版本。
- 运行可解释：任一节点都能定位输入、输出、耗时、错误和依赖。
- 状态可恢复：Worker 销毁后不影响等待或继续执行。
- 权限随 Workflow：模型、工具、Skill、知识和记忆授权给运行身份。
- 调试不污染生产：Pin Data、Mock 和参数覆盖只作用于测试运行。
- 历史不被改写：重新执行创建派生记录。
