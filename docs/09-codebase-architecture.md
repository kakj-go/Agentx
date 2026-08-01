# 代码仓库架构

## 1. Monorepo

Agentx 使用单仓库管理 Rust 服务、公共 Crate、前端、容器和 Kubernetes 清单。

    Agentx/
    ├── apps/
    │   └── web/
    ├── services/
    │   ├── platform-api/
    │   ├── trigger-gateway/
    │   ├── workflow-coordinator/
    │   ├── workflow-worker/
    │   ├── sandbox-manager/
    │   └── trace-writer/
    ├── crates/
    │   ├── agentx-domain/
    │   ├── agentx-application/
    │   ├── agentx-runtime/
    │   ├── agentx-node-sdk/
    │   ├── agentx-api-types/
    │   ├── agentx-infrastructure/
    │   └── agentx-service-kit/
    ├── deploy/
    │   ├── docker/
    │   └── k8s/
    ├── scripts/
    └── docs/

## 2. 后端服务

### platform-api

控制面 API，承载租户、权限、Workflow 管理、资源管理、审批、应用、会话和评测等控制面用例。

### trigger-gateway

生产调用入口，承载 Application API、Webhook、SSE、幂等键、输入校验和 Execution 创建请求。

### workflow-coordinator

负责 Workflow 状态推进、Ready Node 计算、Join、Loop、等待恢复、超时和任务回收。

### workflow-worker

消费节点任务、获取 Lease、解析表达式、调用 Node Runner、保存结果、创建 Checkpoint 和发送 Trace。

### sandbox-manager

隔离 CubeSandbox 的生命周期和协议，为 Worker 提供稳定的沙箱执行接口。

### trace-writer

从 Trace Queue 批量写入 ClickHouse。ClickHouse 故障不能影响 Workflow 的权威状态。

## 3. 公共 Crate

### agentx-domain

纯领域对象和值类型，不依赖 Axum、SQLx、Redis 或 ClickHouse。

### agentx-application

业务用例和 Port。Repository、Queue、Trace、Sandbox、Model 和 Tool 都以接口形式存在。

### agentx-runtime

Workflow 运行内核，包括 Item、图结构、连接类型、编译 IR、状态迁移、Merge 和 Loop。

### agentx-node-sdk

节点定义和执行协议，包括 Node Context、Item、Node Runner 和 Node Error。

### agentx-api-types

外部 HTTP 和内部通信的稳定 DTO，不包含数据库实体。

### agentx-infrastructure

实现 MySQL、Redis、ClickHouse、MinIO、CubeSandbox、LightRAG 和 Mem0 等 Adapter。

### agentx-service-kit

服务启动的公共能力，包括配置、结构化日志、健康检查和优雅退出。

## 4. 依赖方向

    agentx-domain
          ↑
    agentx-application
          ↑
    agentx-runtime / agentx-infrastructure
          ↑
        services

约束：

- services 之间不得直接依赖。
- domain 不得依赖基础设施。
- infrastructure 实现 application 中的 Port。
- runtime 可以依赖 domain 和 node-sdk，不依赖具体数据库。
- API DTO 和数据库实体分离。
- 公共代码只有被两个以上模块稳定复用后才能进入 Crate。

## 5. 服务启动

每个服务是独立 Cargo Package 和二进制，首期统一提供：

- GET /health/live
- GET /health/ready
- JSON 结构化日志
- AGENTX_BIND_ADDR
- Ctrl+C 和 SIGTERM 优雅退出

服务启动不要求所有外部依赖已经可用。接入基础设施后，通过后台重试恢复连接，并由 Readiness 控制是否接收流量。

## 6. 配置

环境变量统一使用 AGENTX 前缀。普通配置来自 ConfigMap，敏感配置来自 Secret。

建议后续按服务建立强类型配置结构，而不是在业务代码中随处读取环境变量。

## 7. 文件组织

服务内部推荐：

    src/
    ├── main.rs
    ├── config.rs
    ├── state.rs
    ├── routes/
    ├── handlers/
    └── modules/

Crate 内部按领域拆分模块。任何单个前端或后端源文件不得超过 2000 行。

