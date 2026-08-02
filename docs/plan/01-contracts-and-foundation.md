# 阶段 01：契约和公共基础

## 1. 目标与用户价值

建立所有业务模块共用的 API、配置、Migration、租户上下文和基础设施 Port，避免每个 Feature 自行定义错误、分页、事务或依赖检查。

## 2. 当前状态和进入条件

- 状态：`done`。
- 进入条件：[阶段 00](00-baseline-and-decisions.md) 完成。
- MySQL、Redis、ClickHouse、MinIO Adapter、Migration、统一 API Client、Artifact 和 Outbox 已接入真实实现。

## 3. 范围和不做内容

本阶段实现公共契约和真实基础设施连接，不实现具体 IAM、Workflow、资源或运行业务。不在公共 Crate 中提前放入只有单个模块使用的业务代码。

## 4. 领域对象和不变量

- UUIDv7 作为业务主键，ID 类型保持强类型。
- `TenantContext`、`ActorContext`、`RequestContext` 由认证中间件建立，业务 Handler 不接受客户端自行声明的可信租户。
- 写模型包含 `created_at`、`updated_at` 和乐观锁版本；需要审计的对象包含创建人与更新人。
- 控制面列表使用页码分页；Trace、Message 和事件流使用游标分页。
- API 错误至少包含稳定 `code`、用户可读 `message`、`requestId` 和可选字段错误。

## 5. 数据和 Migration

- 建立 MySQL 与 ClickHouse 独立 Migration 目录和执行记录。
- 建立数据库连接、事务、分页和健康检查公共实现。
- 建立 Artifact 元数据表、对象存储 Bucket 初始化和引用 Port。
- 建立 Outbox 基础表和 Dispatcher 接口，具体事件在后续阶段增加。
- Migration 必须向前执行；生产清单不自动执行破坏性回滚。

## 6. API、Port 和事件

- 外部 API 统一前缀 `/api/v1`，健康检查继续使用 `/health/live` 和 `/health/ready`。
- OpenAPI 是外部 HTTP 契约来源，生成 TypeScript 类型和客户端。
- `Repository`、`TransactionManager`、`Outbox`、`ArtifactStore`、`Clock` 和 `IdGenerator` 定义在 Application 层。
- Infrastructure 层实现 MySQL、Redis、ClickHouse 和 MinIO Adapter。
- 内部命令和事件必须携带 tenant_id、request_id、occurred_at 和幂等标识。

## 7. 代码和前端边界

- `agentx-domain` 保存纯值类型和不变量。
- `agentx-application` 保存用例与 Port。
- `agentx-api-types` 保存稳定 HTTP 和内部 DTO。
- `agentx-infrastructure` 保存基础设施 Adapter。
- `agentx-service-kit` 提供配置、请求上下文、健康状态和优雅退出。
- 前端新增统一 API Client、认证 Token 注入、错误映射和 TanStack Query Provider；业务 Feature 不直接调用 `fetch`。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| FND-001 | done | DEC-001–022 | UUID、UTC、错误、分页、并发和命名规范 | 契约测试覆盖序列化和错误映射 |
| FND-002 | done | FND-001 | 强类型服务配置与环境变量校验 | 缺失必填配置时启动失败且错误明确 |
| FND-003 | done | FND-002 | MySQL Pool、SQLx Migration Runner 和事务实现 | 空库可初始化，重复执行无副作用 |
| FND-004 | done | FND-002 | Redis、ClickHouse、MinIO Client 与重连策略 | 依赖恢复后无需重启服务即可 Ready |
| FND-005 | done | FND-003 | Artifact Repository、Object Store 和 Bucket 初始化 | JSON 与 Binary 可写入、读取并校验 Hash |
| FND-006 | done | FND-003 | Outbox 表、Port、租约领取、成功确认和失败重试 Adapter | 事务回滚不产生事件，事件 ID 幂等，过期租约可恢复且旧租约不能确认 |
| FND-007 | done | FND-001 | Request ID、API Error、分页和乐观锁中间件 | HTTP 契约测试覆盖成功、校验、冲突和内部错误 |
| FND-008 | done | FND-007 | OpenAPI 输出和 TypeScript Client 生成流程 | CI 能检测 API Schema 与生成类型漂移 |
| FND-009 | done | FND-003–004 | 真实 Readiness、依赖状态和优雅停止 | Liveness 只反映进程，Readiness 反映必要依赖 |
| FND-010 | done | FND-003–009 | Testcontainers 集成测试和测试数据隔离 | 测试可重复执行且不依赖开发者已有数据 |
| FND-011 | done | FND-008 | 前端 API Client、Query Provider 和统一错误展示 | Feature 可通过生成契约显示加载、错误和空状态 |

## 9. 失败、安全和幂等边界

- 客户端不能通过 Header 覆盖已认证 Tenant Context。
- 日志不得记录 Token、Cookie、Credential 或请求原文中的 Secret。
- 数据库超时映射为稳定错误码，不暴露 SQL 和内部地址。
- Outbox Dispatcher 采用至少一次投递，消费者承担幂等。
- 对象写入成功但事务失败时产生可清理孤儿对象，不创建有效 Artifact 引用。

## 10. 测试

- Domain ID、时间和错误序列化单元测试。
- MySQL Migration、事务、乐观锁和分页集成测试。
- Redis 重连、Outbox 重复投递和 MinIO Hash 校验测试。
- OpenAPI Snapshot 与前端生成类型检查。
- Kubernetes 依赖缺失、恢复和优雅停止测试。

## 11. 验收门禁

- 前后端通过同一 OpenAPI 契约通信。
- Migration 支持空库和已存在旧版本数据库。
- 所有业务请求具备 Request ID；受保护请求可建立 Tenant Context。
- 基础设施不可用时 Readiness 正确失败，恢复后自动转为 Ready。
- 公共接口没有依赖具体业务 Feature。

## 12. 对后续阶段的稳定输出

- `/api/v1`、错误、分页、乐观锁和 OpenAPI 规则。
- MySQL、Redis、ClickHouse、MinIO Adapter 基础。
- Transaction、Outbox、Artifact 和测试基础设施。
- 前端统一 API Client 与 Query 基础。
