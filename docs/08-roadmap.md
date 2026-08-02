# 实施路线与验收标准

## 0. 阶段零：工程骨架

交付：

- Rust Cargo Workspace
- 独立 services 目录
- 公共 crates 目录
- React、TypeScript、Tailwind CSS 前端
- 企业工作台 App Shell
- React Flow 示例画布
- Docker 镜像定义
- 本地 Kubernetes 应用和中间件
- 精简 README 和 Apache-2.0 LICENSE

验收：

- Cargo Workspace 可以编译。
- 前端可以完成生产构建。
- 每个后端服务拥有独立进程和健康检查。
- Kustomize 可以生成合法清单。
- 本地镜像构建后可以启动全部工作负载。

## 1. 阶段一：Workflow 内核

预计 4 至 6 周。

交付：

- Workflow Definition
- Node Definition
- Item 数据模型
- 受限表达式
- Workflow 编译 IR
- DAG 调度
- IF、Switch、Merge、Loop
- Execution、Node Execution、Node Attempt
- Redis Queue
- Lease、Heartbeat 和 Reaper
- 基础 Checkpoint

验收：

- 多分支和循环 Workflow 能正确执行。
- Worker 在节点运行中退出后，任务能够恢复或重试。
- 重复队列消息不会无条件重复运行节点。
- 每个节点输入输出可查询。

## 2. 阶段二：Workflow Studio

预计 5 至 7 周。

交付：

- React Flow 画布
- 节点面板
- 参数表单
- 表达式编辑
- 自动保存
- Draft Revision
- 手动运行
- Execute Node
- Execute To Node
- Pin Data
- 画布运行高亮
- Workflow Version

验收：

- 用户能够通过拖拽完成基础 Workflow。
- 调试体验接近 n8n。
- 发布版本不可修改。
- 草稿和历史版本可以比较。

## 3. 阶段三：Agent 和 CubeSandbox

预计 5 至 7 周。

交付：

- Model、Agent、Tool 节点
- Skill Definition、不可变 Skill Version 和依赖清单
- Skill Workflow Grant 和 Agent Runtime 加载
- ai_model、ai_tool、ai_memory 和 ai_retriever 连接
- Agent Iteration
- Tool 授权检查
- CubeSandbox Manager
- Python、JavaScript、Shell 节点
- Token 和成本
- 工具错误
- Agent 循环限制
- ClickHouse Trace

验收：

- Agent 能在授权范围内调用多个 Tool。
- Agent 只能加载已授权 Skill，且 Skill 依赖仍需通过各资源权限检查。
- 可以查看每次模型请求和 Tool 调用。
- 达到循环、成本或时间限制时能够终止或进入错误分支。
- Code 节点只在 CubeSandbox 中运行。

## 4. 阶段四：发布、应用和会话

预计 4 至 6 周。

交付：

- Workflow Deployment
- Application
- Session
- Message
- API Key
- SSE
- Webhook
- Schedule
- Playground
- 版本切换和回滚

验收：

- Workflow 可以发布为外部应用。
- 每轮消息生成独立 Execution。
- Playground 与正式 API 使用相同运行链路。
- 历史 Session 能定位到运行时 Workflow Version。

## 5. 阶段五：Checkpoint 和审批

预计 4 至 5 周。

交付：

- 完整 Checkpoint
- Fork Execution
- 从任意可恢复节点重新执行
- 副作用节点保护
- Wait Node
- Approval Node
- Approval Task
- 待办和站内通知
- 超时和恢复

验收：

- Execution 可以等待较长时间且不占用 Worker。
- 审批完成后可以由任意 Coordinator 恢复。
- 用户可以从历史节点创建新的派生执行。
- 原 Execution 不被修改。

## 6. 阶段六：测试和评测

预计 4 至 6 周。

交付：

- Dataset 和 Dataset Version
- Test Case
- Evaluation Run
- 批量执行
- 内置 Evaluator
- LLM Judge
- 版本对比
- 评测报告

验收：

- 一个 Workflow Version 可以批量运行测试集。
- 报告可以展示成功率、成本、耗时和工具错误。
- 失败 Case 可以跳转到完整 Trace。
- 可以对比两个 Workflow Version。

## 7. MVP 验收闭环

MVP 完成时，用户应能完成：

1. 初始化企业和 Admin。
2. 创建部门、用户和角色。
3. 接入模型、Tool、Skill、LightRAG 和 Mem0。
4. 将资源授权给 Workflow。
5. 拖拽创建包含 Agent、Tool、Code 和审批的 Workflow。
6. 手动运行并查看节点输入输出。
7. 在 ClickHouse Trace 页面查看模型、Tool 和成本。
8. 从历史 Checkpoint 重新执行。
9. 用 Dataset 批量评测版本。
10. 发布为 Application。
11. 通过 Playground 和 API 建立 Session 并发送消息。
12. 在待办页面审批并恢复 Workflow。

## 8. 开发优先级

最高优先级：

1. Item、Node 和表达式语义。
2. 可恢复的 Scheduler。
3. Version、Execution 和 Node Execution 数据模型。
4. Agent Tool Loop。
5. Trace 和 Checkpoint 数据贯通。

第二优先级：

1. 画布交互完善。
2. 应用和会话。
3. 审批。
4. 测试评测。
5. 更多节点。

暂缓：

- 大量第三方连接器
- n8n 全量节点兼容
- 通用 BPMN
- 插件交易市场
- 通用监控平台
- 跨区域多活

## 9. 编码前必须确定的决策

- n8n JSON 是否要求导入，兼容到什么程度。
- Workflow IR 的最终 Schema。
- 表达式语法是否兼容 n8n 常用表达式。
- Redis Streams 是否作为首期唯一任务队列。
- 节点输出多大后转为 Artifact。
- Checkpoint 默认粒度。
- Sub-workflow 固定版本还是跟随 Deployment。
- Session 默认固定版本还是跟随当前 Deployment。
- Code 节点允许的语言和 CubeSandbox 模板。
- Trace 中 Prompt 和 Response 的默认保存策略。
