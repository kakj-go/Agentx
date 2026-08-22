# 代码仓库架构

## 1. Monorepo

Agentx 使用单仓库管理 Rust 服务、公共 Crate、前端、容器、分域 Migration、Helm Chart、可选 Kustomize Addon 和跨平台 Python 运维工具。

```text
Agentx/
├── apps/
│   ├── web/
│   └── e2e/
├── services/
│   ├── platform-control/
│   ├── agentx-v2-runtime/          # runtime-gateway/workflow-runtime/worker/sandbox binaries
│   ├── observability/
│   ├── echo-mcp/                   # E2E Provider Fixture
│   └── echo-node/                  # E2E Node Fixture
├── crates/
│   ├── agentx-domain/
│   ├── agentx-application/
│   ├── agentx-runtime/
│   ├── agentx-node-protocol/
│   ├── agentx-api-types/
│   ├── agentx-runtime-contracts/
│   ├── agentx-bundle-builder/
│   ├── agentx-control-infrastructure/
│   ├── agentx-runtime-infrastructure/
│   ├── agentx-mysql-lease/
│   ├── agentx-service-kit/
│   ├── agentx-boundary-check/
│   └── agentx-v2-ops/
├── migrations/{control,runtime,observability}/
├── deploy/{docker,helm,kustomize,opensandbox,python,values}/
├── tests/e2e/
├── tools/python/
└── docs/
```

V1 `platform-api`、`trigger-gateway`、`workflow-coordinator`、`trace-writer`、旧 Worker/Sandbox 服务、`agentx-runtime-rpc`、共享 Infrastructure 和 `migrations/mysql` 已物理删除。

## 2. 后端构建产物

### platform-control

唯一承载 `/api/v1`，以 `api,publisher,projector,retention` Role 处理 IAM、Workflow、资源、应用发布、治理投影和 Runtime/Observability BFF。它只访问 Control MySQL/OSS、只读 Vault 和冻结 Internal API。

### agentx-v2-runtime

一个 Package 提供四个 Runtime 二进制：

- `runtime-gateway`：`/gateway/v1`、Session/Message/Invocation、Webhook、SSE、Cancel/Resume 和 Runtime Query。
- `workflow-runtime`：Coordinator、Trigger、Command、Outbox、Recovery、Artifact、Quota 和 Trace Relay Role。
- `workflow-worker`：按 Capability 消费任务，使用 Attempt Lease/Fencing 执行 Node、Resource 和 Provider 调用。
- `sandbox-manager`：OpenSandbox 生命周期、短凭据、Reaper、TTL 和 Provider 对账。

四个二进制只使用 Runtime MySQL/Redis/OSS、只读 Vault 和获准 Provider/OpenSandbox，不访问 Control Repository 或数据。

### agentx-egress-gateway

独立 Rust 二进制，提供 Runtime `3128` 明文集群内 CONNECT、Sandbox `3129` TLS CONNECT、`8080` 健康检查和 `9092` 低基数指标。它强制 KID/角色绑定、Runtime Token 单次消费、Sandbox Token 并发/次数/累计时长预算，以及解析后全地址校验和固定 IP 连接。它只依赖跨服务 JWT 契约、DNS/TCP/TLS 与 Service Kit，不依赖任何业务 Repository 或数据凭据。

### observability

以 `trace-consumer,query` Role 消费 Runtime Trace Stream、写入 ClickHouse并提供内部 Trace/成本/聚合查询。它不持有 Control 或 Runtime MySQL Credential。

## 3. 公共 Crate

### 纯领域与协议

- `agentx-domain`：纯领域对象和值类型，不依赖 Axum、SQLx、Redis 或 ClickHouse。
- `agentx-runtime`：Workflow Item、图、表达式、编译 IR 和状态机算法；不依赖具体存储。
- `agentx-node-protocol`：版本化 Node Manifest、Action/Lifecycle、Item/Lineage、Artifact、Credential Handle 和错误 DTO。
- `agentx-runtime-contracts`：跨面 Bundle、Work Package、Command/Event、Internal API、IR、Worker Protocol 和 JWT DTO；使用严格版本与未知字段拒绝。
- `agentx-api-types`：公共 HTTP DTO，不包含数据库 Row。

### 应用与基础设施

- `agentx-application`：存储无关的业务 Port/Use Case。
- `agentx-bundle-builder`：确定性 Bundle/Work Package/对象闭包构建和签名。
- `agentx-control-infrastructure`：Control MySQL/OSS、Credential、Artifact 和 Outbox Adapter。
- `agentx-runtime-infrastructure`：Runtime MySQL/Redis/OSS、Vault 和 Provider Adapter。
- `agentx-mysql-lease`：数据库 UTC 时间、Pod UID Owner、Lease、Heartbeat、Fencing 和 `SKIP LOCKED` 公共语义。
- `agentx-service-kit`：配置、结构化日志、Live/Ready/Drain、指标和优雅退出。
- `agentx-v2-ops`：Migration、Bootstrap、Doctor 和 Key 工具。
- `agentx-boundary-check`：Cargo、SQL、Env、Secret、NetworkPolicy、Provider HTTP Client、V1 残留和 2000 行静态门禁；面向公网的 Runtime 模块禁止重新使用裸 `reqwest::Client::new()`。

## 4. 依赖方向

```text
agentx-domain / agentx-node-protocol
                ↑
      agentx-runtime-contracts
          ↑              ↑
 agentx-runtime   agentx-application
          ↑              ↑
 runtime-infra      control-infra
          ↑              ↑
 Runtime services   platform-control

observability → runtime-contracts / ClickHouse client
```

约束：

- 服务之间不得通过 Cargo 直接依赖，也不能共享 Repository 绕过 Internal API。
- Domain/Runtime Kernel 不依赖数据库、Redis、ClickHouse 或服务代码。
- Control Infrastructure 不依赖 Runtime Infrastructure、Redis 或 ClickHouse；Runtime Infrastructure 不依赖 Control Infrastructure。
- SQL Row、Settings、Repository 和数据库凭据不跨 Plane 导出。
- 跨面只共享版本化 Contracts；Control→Runtime/Observability 使用短期 RS256 Service/Delegation JWT。
- Runtime 不主动调用 Control，Observability 不持有任何 MySQL Credential。

## 5. 服务生命周期和配置

六类后端应用统一提供：

```text
GET  /health/live
GET  /health/ready
POST :9091/health/drain
GET  :9092/metrics
```

普通配置来自分面 ConfigMap，敏感配置来自工作负载 Secret，环境变量统一使用 `AGENTX_` 前缀。Drain 后立即摘除 Readiness并停止新写/新 Claim；已领取工作在限定时间内 Heartbeat，Kubernetes 终止宽限为 60 秒。

## 6. 文件组织与门禁

服务按 Auth/IAM、Workflow、Resource、Application、Evaluation、Governance、Runtime Engine、Query 和 Provider 等领域拆分模块。任何前端或后端生产源文件不得超过 2000 行；生成文件和 Migration 必须进入明确允许列表。

当前完成事实和数据访问图见 [当前架构、服务与数据访问图](13-architecture-service-data-map.md)，本地收口证据见 [V2-08A 验收](planv2/evidence/v2-08.md)。生产容量、安全与恢复认证仍属于 V2-08B。
