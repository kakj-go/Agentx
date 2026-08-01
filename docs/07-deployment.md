# Kubernetes 部署与扩展

## 1. 首期部署拓扑

工作负载：

- web-console
- platform-api
- trigger-gateway
- workflow-coordinator
- workflow-worker
- sandbox-manager
- trace-writer

中间件：

- MySQL
- Redis
- ClickHouse
- MinIO 或兼容 S3
- CubeSandbox

本地 Docker Desktop Kubernetes 默认由一套 Kustomize Overlay 启动全部 Agentx 服务以及 MySQL、Redis、ClickHouse 和 MinIO。CubeSandbox 需要满足虚拟化、内核和计算节点要求，不能用普通占位 Pod 代替；本地集群通过 Sandbox Manager 连接单独安装的 CubeSandbox。

## 2. 水平扩展

### platform-api

无状态部署，可直接增加副本。Session 状态不保存在进程内。

### trigger-gateway

无状态部署。SSE 需要通过 executionId 或 streamId 从 Redis 获取事件，避免只能连接到创建请求的实例。

### workflow-coordinator

多个实例通过 MySQL 行锁、状态条件和租约协作。任何 Execution 都不绑定固定 Coordinator。

### workflow-worker

按以下指标扩容：

- Redis Pending 数量
- 最老任务等待时间
- 活跃 Node Execution 数量
- Worker 可用并发

可以使用 Kubernetes HPA 或 KEDA 读取队列指标，不要求搭建 Grafana。

Worker 可按能力拆分池：

- general-worker
- ai-worker
- connector-worker
- sandbox-worker
- evaluation-worker

### sandbox-manager

无状态控制层。实际 Sandbox 资源由 CubeSandbox 管理，可设置租户级并发和预热池。

### trace-writer

按 Trace Queue 积压扩容，批量写 ClickHouse。

## 3. Worker 标签和节点路由

Node Definition 可以声明 capability：

- network-public
- network-internal
- python
- javascript
- browser
- gpu
- high-memory

Scheduler 根据 capability 将任务投递到不同队列，避免普通 Worker 接收无法执行的节点。

## 4. 配置

配置类型：

- 数据库和 Redis 连接
- ClickHouse 连接
- 对象存储
- CubeSandbox
- 加密主密钥
- API 域名
- 默认租户配额
- Trace 保存策略

敏感配置通过 Kubernetes Secret 注入，普通配置通过 ConfigMap。

本地 Overlay 中的 Secret 仅用于开发环境，不得复用到测试或生产环境。

## 5. 第一次启动

系统检测是否完成 Bootstrap：

1. 输入企业名称。
2. 创建首个 Tenant。
3. 创建 Admin 用户。
4. 设置密码。
5. 设置语言和时区。
6. 可选接入首个模型。
7. 标记 Bootstrap 完成。

完成后初始化接口拒绝再次创建管理员。

## 6. 可靠运行

必要机制：

- MySQL Migration Job
- Readiness 和 Liveness Probe
- 优雅停止 Worker
- Worker 停止前不再领取新任务
- Lease 和 Heartbeat
- 过期任务 Reaper
- Outbox Dispatcher
- Trace Queue 重试
- ClickHouse 写入失败不影响 Execution 完成
- Sandbox 超时强制回收

## 7. 租户运行配额

首期只实现和 Workflow 直接相关的配额：

- 并发 Execution
- 并发 Node Execution
- 并发 Sandbox
- 单次 Execution 最大时间
- Agent 最大迭代
- 单次和每日 Token
- 单次和每日成本
- Artifact 大小
- Trace 保留时间

Scheduler 在任务入队和执行前检查配额。

## 8. 环境

至少区分：

- development
- test
- production

Deployment 指向特定环境和 Workflow Version。环境之间不共享 Credential 明文，资源映射通过 Alias 完成。

## 9. 不依赖通用监控产品的产品能力

平台内部仍需要提供基本运行状态页面：

- Worker 在线数量
- Queue 积压
- 当前 Running、Waiting、Failed Execution
- Sandbox 使用数量
- Trace 写入积压

这些数据可由 MySQL、Redis、ClickHouse 和各服务健康接口直接提供。它们属于平台运行管理页面，不要求 Prometheus 或 Grafana。

## 10. 本地 Kubernetes 目录

本地部署使用以下层级：

    deploy/k8s/
    ├── base/
    │   ├── applications/
    │   ├── middleware/
    │   └── namespace.yaml
    ├── overlays/
    │   └── local/
    └── cubesandbox/

Base 保存可复用的应用和中间件清单，local Overlay 生成本地 ConfigMap、Secret 和镜像标签。

应用工作负载：

- web
- platform-api
- trigger-gateway
- workflow-coordinator
- workflow-worker
- sandbox-manager
- trace-writer

本地中间件：

- MySQL StatefulSet 和 PVC
- Redis StatefulSet 和 PVC
- ClickHouse StatefulSet 和 PVC
- MinIO StatefulSet、PVC 和 Bucket 初始化 Job

应用进程必须自行重试外部依赖。Liveness 只表示进程存活，Readiness 在后续接入基础设施适配器后负责确认必要依赖可用。

## 11. CubeSandbox 部署边界

CubeSandbox 官方标准 Kubernetes 部署目前仍处于预览阶段，计算节点需要额外的虚拟化和运行时条件。Agentx 仓库不维护一份简化但不可工作的 CubeSandbox 清单。

本地开发有两种方式：

1. 在符合要求的 Linux 主机或独立 Kubernetes 集群安装 CubeSandbox。
2. 将 AGENTX_CUBESANDBOX_ENDPOINT 指向该 Cube API。

生产环境必须使用 CubeSandbox 官方部署包和经过验证的计算节点。
