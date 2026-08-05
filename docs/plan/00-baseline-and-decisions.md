# 阶段 00：基线与决策记录

## 1. 目标与用户价值

在业务编码前固定首期范围、核心语义和跨阶段边界，让外围能力可以先完成，同时保证后续运行引擎和 Studio 接入时不推翻已有数据。

## 2. 当前状态和进入条件

- 状态：`done`。
- 工程骨架、现有产品文档和前端工作台已完成盘点。
- 本文列出的决策是后续阶段的默认约束，变更必须记录原因、影响和迁移方式。

## 3. 首期范围

首期实现 [产品范围](../01-product-scope.md) 中的“首期必须完成”和 [路线图](../08-roadmap.md) 中的 MVP 闭环。

明确不做：

- 通用 OA、完整 BPMN 和通用页面搭建。
- 大规模 SaaS 连接器市场和插件交易市场。
- 完整商业计费平台。
- Prometheus、Grafana 和通用监控平台。
- 跨区域多活。
- n8n 全量节点或二进制兼容。

## 4. 已确定决策

| 编号 | 决策 | 约束 |
|---|---|---|
| DEC-001 | n8n 只作为语义和体验参考 | Agentx 使用独立 Workflow Definition；JSON 导入后续通过 Adapter 提供 |
| DEC-002 | 最小 Workflow 契约前置 | 完整 Scheduler 和 Studio 后置，稳定 ID、Version、Execution 和 Resource Reference 前置 |
| DEC-003 | MySQL 保存权威状态 | Redis、ClickHouse 和进程内缓存均不得代替 MySQL 决定业务状态 |
| DEC-004 | Redis Streams 是首期唯一运行队列 | Queue 消息允许重复，消费前必须校验 MySQL 状态和 Lease |
| DEC-005 | ClickHouse 只保存 Trace 明细 | Trace 延迟或丢失不能改变 Execution 结果 |
| DEC-006 | 大内容进入对象存储 | Binary、附件、大型 JSON、Checkpoint Payload 和报告通过 Artifact Reference 传递 |
| DEC-007 | 平台认证采用 JWT | Access Token 短时有效并只保存在前端内存；Refresh Token 可轮换并放入 HttpOnly Cookie |
| DEC-008 | 密码使用 Argon2id | 不保存明文、可逆密文或可重复使用的初始密码 |
| DEC-009 | 首期仅预留 OIDC Adapter | OIDC 不属于首期验收门禁，但本地账号不能阻塞后续接入外部身份 |
| DEC-010 | Application 使用独立 API Key | 创建时显示一次，数据库只保存 Hash 和标识前缀 |
| DEC-011 | 生产运行使用 Workflow Service Identity | 不继承设计者个人权限，发布和运行时都校验资源 Grant |
| DEC-012 | Session 创建时固定 Workflow Version | Deployment 切换只影响新 Session；显式升级才改变已有 Session |
| DEC-013 | 生产 Sub-workflow 固定 Version | 编译 Version 时固化子 Workflow Version，不跟随可变 Draft |
| DEC-014 | Version 不可变 | Workflow、Dataset、Tool、Skill 和价格等历史版本不得原地修改 |
| DEC-015 | Checkpoint 默认节点级 | 成功 Node Execution 后形成逻辑 Checkpoint，副作用、Wait 和 Approval 节点前后加强 |
| DEC-016 | 历史执行不改写 | 重试历史节点和 Checkpoint 恢复均创建 Fork Execution |
| DEC-017 | 高风险代码只在 OpenSandbox 执行 | Python、JavaScript、Shell、浏览器自动化和动态代码不得在 Worker 进程直接执行 |
| DEC-018 | Playground 使用正式 Application API | 不建立仅供前端测试的第二条运行路径 |
| DEC-019 | 运行不可用必须显式失败 | 引擎接入前返回 `RUNTIME_UNAVAILABLE`，不得创建伪成功 Execution、Approval 或 Evaluation |
| DEC-020 | 前端保持统一组件体系 | Tailwind 语义 Token、Radix 行为原语和共享组件是唯一界面基础 |
| DEC-021 | 首期采用单公司部署 | 保留 tenant_id，但不开放 Tenant CRUD 或切换；Company Admin 管理全公司，Department Admin 管理部门子树 |
| DEC-022 | 新用户使用临时密码 | 管理员设置临时密码，首次登录必须修改后才签发正常 Refresh Session |
| DEC-023 | 沙箱实现切换为 OpenSandbox | Agentx 保留通用 `SandboxRuntime` 和独立 `sandbox-manager`；本地使用 Docker Runtime + runc，生产必须使用 Kubernetes Runtime + gVisor/Kata 或经安全评审的等价强隔离 Runtime |
| DEC-024 | OpenSandbox 使用 Rust 直接 Adapter | `sandbox-manager` 通过 Rust HTTP/SSE Client 直接调用固定版本的 Lifecycle/execd API；OpenAPI 生成物只用于内部 DTO 和普通 HTTP 端点，SSE、Endpoint 校验、重试和错误映射由手写层负责；生产链路不增加 Go/Python Sidecar |

2026-08-04 决策补充：DEC-023/024 取代原沙箱接入方向。影响范围为阶段 10、`sandbox-manager`、`agentx-infrastructure`、本地部署和生产隔离门禁；当前尚无已发布 Sandbox 数据需要迁移。OpenSandbox 协议升级必须显式更新固定的 Spec Commit/Hash、组件版本、生成物和契约证据，不能通过宽松反序列化静默兼容。

## 5. 稳定公共概念

后续阶段必须保持以下职责不变：

- `TenantContext`：当前租户、用户、角色、部门和授权范围。
- `WorkflowServiceIdentity`：生产 Workflow 的运行身份。
- `WorkflowId`、`WorkflowVersionId`、`DeploymentId`、`ExecutionId`：UUIDv7 稳定标识。
- `ResourceReference`：资源类型、资源 ID 和可选的不可变版本 ID。
- `ResourceGrant`：Workflow 或部门对资源的显式授权。
- `WorkflowDefinition`：独立于 React Flow 和 n8n JSON 的可版本化定义。
- `CompiledWorkflow`：后端编译并供运行时消费的 IR。
- `ExecutionCommand`：创建、取消、恢复和 Fork 执行的命令边界。
- `ArtifactReference`：跨数据库和对象存储的稳定大对象引用。

## 6. 版本和兼容规则

- 外部 REST API 使用 `/api/v1`。
- API DTO 与数据库实体分离。
- 时间统一保存 UTC，API 使用带时区的 ISO 8601。
- JSON Definition 和 Manifest 必须包含 Schema Version。
- 新代码必须能读取当前受支持的历史 Schema；破坏性变化通过迁移器产生新版本。
- React Flow Node/Edge 只属于编辑状态，不能直接作为运行协议。

## 7. Trace 和敏感数据原则

- 手动测试和生产运行都保留完整的结构化 Trace 层级。
- Secret、Authorization Header、Cookie 和 Credential 明文在写入 Trace 前必须脱敏。
- 大型 Prompt、Response、Tool Result 和文件写入对象存储，ClickHouse 保存摘要和引用。
- 租户可以配置内容保留时间和是否保存完整 Prompt/Response，但至少保留状态、耗时、Token、成本和错误摘要。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| PLN-001 | done | — | 代码、前端、部署和文档基线盘点 | 当前完成与未完成边界可由仓库内容验证 |
| PLN-002 | done | PLN-001 | 首期范围、排除项和实施顺序 | 与产品范围一致且没有把暂缓能力纳入门禁 |
| PLN-003 | done | PLN-002 | DEC-001–024 和稳定公共概念 | 后续阶段均引用相同版本、认证、存储和运行边界 |
| PLN-004 | done | PLN-003 | 阶段计划、任务规则和追踪矩阵 | 首期能力与 MVP 十二步均有唯一验收归属 |

## 9. 失败、安全和变更边界

- 未经记录不得改变 Tenant、Version、Execution、Resource Grant 或 Secret 的安全语义。
- 新实现发现决策不可行时应阻塞对应任务，不能通过局部例外绕过全局不变量。
- 决策记录不得包含 Secret、生产地址、客户数据或可执行 Credential。

决策变更流程：

变更任一 `DEC-*` 时必须：

1. 在本文追加变更日期、原因和替代方案。
2. 标出受影响的阶段、表、API 和历史数据。
3. 说明兼容和 Migration 方式。
4. 先更新原始架构文档，再更新实施计划和追踪矩阵。
5. 不允许仅通过代码提交隐式改变架构边界。

## 10. 测试

- 检查产品范围、路线图、阶段计划和追踪矩阵的功能集合一致。
- 检查所有阶段引用的 `DEC-*` 存在且含义唯一。
- 检查 Markdown 相对链接、任务编号和状态值。
- 变更决策时以至少一个失败场景验证迁移或兼容方案。

## 11. 完成门禁

- 首期范围无互相冲突的描述。
- 所有后续阶段都引用本文稳定边界。
- 原路线图中的关键未决项已经在本文确定或归入明确阶段的 Schema 评审任务。
- OIDC、n8n JSON 导入和通用监控未进入首期验收门禁。

## 12. 对后续阶段的稳定输出

- 首期范围和排除项。
- 全局不变量和公共概念。
- 认证、版本、存储、运行和 UI 的默认决策。
- 架构决策变更流程。
