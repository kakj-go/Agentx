# 系统架构

## 1. 业务分层

### Workflow Studio

面向设计者和测试者：

- Workflow 画布
- 节点配置面板
- 表达式编辑器
- 手动执行
- 单节点执行
- Pin Data
- 执行路径高亮
- Trace 和 Checkpoint 查看
- Playground
- Dataset 和评测报告

### Workflow 控制面

管理静态配置和业务资源：

- Tenant、Department、User、Role
- Workflow、Draft、Version、Deployment
- Node Definition 和 Node Version
- Model、Tool、RAG、Memory、Credential
- Application、Session、Message
- Approval Task 和 Notification
- Dataset、Evaluator、Evaluation Run

### Workflow 运行面

处理所有运行状态：

- Trigger Gateway
- Execution Coordinator
- Scheduler
- Redis Queue
- Workflow Worker
- Node Runner
- CubeSandbox Manager
- Trace Writer

### 数据层

- MySQL：权威业务数据和运行状态
- Redis：任务派发、缓存、租约、短期状态和限流
- ClickHouse：Workflow Trace 和分析明细
- MinIO 或 S3：文件、二进制输出、大型节点结果、Checkpoint Payload
- CubeSandbox：不可信代码和高风险工具运行环境

## 2. 推荐部署单元

### web-console

承载管理控制台、Workflow Studio、Playground、Trace 和评测页面。

### platform-api

首期采用模块化单体，包含：

- IAM
- Workflow 管理
- 资源管理
- 应用和会话
- 审批和通知
- 测试集和报告
- ClickHouse 查询 API

### trigger-gateway

负责：

- Application API
- Webhook
- SSE 连接
- 请求认证
- 输入 Schema 校验
- 幂等键
- 创建 Execution 请求

### workflow-coordinator

负责：

- 创建 Execution
- 固化 Execution Snapshot
- 计算初始节点
- 推进节点状态
- Join 和 Loop 计算
- Wait 和 Approval 恢复
- 超时、取消和故障任务回收

### workflow-worker

负责：

- 消费节点任务
- 取得节点租约
- 解析表达式
- 加载 Credential
- 执行具体 Node Runner
- 保存节点结果
- 创建 Checkpoint
- 发送 Trace Event

### sandbox-manager

负责 CubeSandbox 生命周期、资源配额、输入输出和短期凭证注入。

### trace-writer

从 Redis Stream 或专用 Trace Queue 批量写入 ClickHouse。ClickHouse 暂时不可用时，Workflow 执行不能因此失败。

## 3. 模块依赖规则

- Studio 只能通过 Platform API 和 Trigger Gateway 调用后端。
- Platform API 不直接执行节点。
- Worker 不修改 Workflow Draft。
- Execution 只能运行 Version Snapshot，不能直接运行实时变化的生产草稿。
- Scheduler 不依赖进程内状态判断工作流进度。
- Redis 中的消息不是权威状态，消费前必须校验 MySQL。
- ClickHouse 丢失或延迟不能影响 Execution 状态正确性。
- 大型 Payload 通过 Artifact Reference 传递，避免数据库行无限增长。

## 4. 技术栈建议

结合当前 Rust 项目：

- 后端：Rust、Axum、Tokio、SQLx
- 内部通信：gRPC
- 外部调用：REST、SSE、Webhook
- 前端：React、TypeScript、Tailwind CSS、Radix UI、React Flow
- 前端状态：TanStack Query、Zustand
- 表单与校验：React Hook Form、Zod
- 数据库：MySQL 8
- 队列和短期状态：Redis Streams
- Trace：ClickHouse
- 文件和大对象：MinIO 或 S3
- 沙箱：TencentCloud CubeSandbox
- 部署：Kubernetes、Kustomize；成熟后再评估 Helm

Node SDK 可以支持 Rust、Python 和 JavaScript。Python 与 JavaScript 自定义代码优先放在 CubeSandbox 内执行，不要求所有节点都由 Rust 编写。

## 5. 一致性边界

MySQL 事务负责：

- Execution 状态变化
- Node Execution 状态变化
- 下游节点 Ready 判定
- Checkpoint 元数据
- Outbox 事件

Outbox Dispatcher 将事务内事件投递到 Redis。即使 Redis 投递重复，Worker 也通过节点状态和 Lease 保证不会无条件重复执行。

外部副作用无法获得通用 Exactly Once 保证。平台提供：

- Idempotency Key
- At-least-once 调度
- 节点副作用标记
- 重试前确认
- Dry Run
- 补偿分支
