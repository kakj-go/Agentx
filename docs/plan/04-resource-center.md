# 阶段 04：资源中心

## 1. 目标与用户价值

让租户集中接入并授权模型、工具、Skill、LightRAG、Mem0 和 Credential，确保设计时可选择、发布时可固化、运行时可二次校验。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：[阶段 03](03-workflow-control-plane.md) 的 Workflow、Service Identity、成员与 Grant 主体边界完成；Version 的最终资源快照和发布校验在本阶段资源模型稳定后共同收口。
- 当前资源页面均为 Mock，没有 Credential 页面和连接测试。
- M2 实际开发顺序、阶段交叉依赖和子任务见 [M2 实施任务清单](m2-task-list.md)。

## 3. 范围和不做内容

实现资源控制面、版本、连接健康、依赖和授权。不在本阶段执行模型、Tool、Skill 代码、RAG 查询或 Memory 写入；运行 Adapter 在阶段 10 接入。

## 4. 领域对象、状态和不变量

- Credential 是 Secret 或外部 Secret Reference，API 永不返回可恢复明文。
- Model Alias 是 Workflow 推荐引用，Model Deployment 保存 Provider、Endpoint 和 Credential Reference。
- Model Price Version 不可变，历史成本按调用发生时版本计算。
- Tool Definition 可变，Tool Version 不可变并固化输入输出 Schema、副作用、超时和 Runner 类型。
- Skill Definition 可变，Skill Version 不可变并固化 Manifest、Content Hash、Artifact 和依赖。
- RAG 和 Memory 授权区分读写范围。
- Grant 可以授予 Department 或 Workflow；运行时最终使用 Workflow Service Identity 检查。

## 5. 数据和 Migration

主要表：

- credentials、credential_secret_versions
- model_providers、model_deployments、model_aliases、model_price_versions
- tools、tool_versions
- skills、skill_versions、skill_dependencies
- rag_connections、rag_resources
- memory_connections、memory_namespaces
- resource_health_checks、resource_grants、workflow_resource_grants、workflow_skill_grants

Secret 使用版本化密文或 Kubernetes/外部 Secret Reference；本地密文由平台主密钥保护，并保存算法版本、Nonce 和轮换信息。

## 6. REST API、Port 和事件

- `/api/v1/credentials`
- `/api/v1/models/providers|deployments|aliases|prices`
- `/api/v1/tools` 和 `/versions`
- `/api/v1/skills`、`/versions` 和 `/dependencies`
- `/api/v1/knowledge/connections|resources`
- `/api/v1/memory/connections|namespaces`
- `/api/v1/resources/{type}/{id}/grants`
- 统一 `POST .../test-connection`

定义 `CredentialResolver`、`ResourceAuthorizer` 和只做连接验证的控制面 Adapter；运行 Port 由阶段 10 扩展。

## 7. 后端和前端改动

- Platform API 增加 credentials、models、tools、skills、knowledge、memory 和 grants 模块。
- Resource Authorizer 接收 Tenant、Workflow Service Identity、资源、版本和操作类型。
- 前端新增 Credential 管理入口和各资源详情、版本、连接测试、依赖及授权界面。
- 当前 `/models`、`/tools`、`/skills`、`/knowledge` 和 `/memory` 列表替换 Mock。
- 所有资源选择器只返回当前用户可见且目标 Workflow 可用的资源。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| RES-001 | planned | FND-005、IAM-005 | Credential Schema、加密、外部引用和轮换 | API 与日志均不暴露明文，旧版本可受控轮换 |
| RES-002 | planned | RES-001、WCP-006 | Model Provider、Deployment、Alias 和 Price Version | Alias 可切换 Deployment，历史价格版本不可改 |
| RES-003 | planned | RES-001 | Tool Definition、Version、Schema 和副作用配置 | 发布后的 Tool Version 不可变且 Schema 可校验 |
| RES-004 | planned | RES-003、FND-005 | Skill Definition、Version、Manifest、Artifact 和依赖 | Content Hash 可复现，循环依赖被拒绝 |
| RES-005 | planned | RES-001 | LightRAG Connection、Resource 和读写范围 | Connection Test 不保存查询内容到日志 |
| RES-006 | planned | RES-001 | Mem0 Connection、Namespace 和读写范围 | Namespace 跨租户引用被拒绝 |
| RES-007 | planned | RES-002–006、WCP-006 | Department/Workflow Grant 和 Resource Authorizer | Skill Grant 不传递依赖资源权限 |
| RES-008 | planned | RES-002–007 | 统一连接测试、健康状态和审计事件 | 测试超时可取消，失败不会泄露 Secret |
| RES-009 | planned | RES-001–008 | Resource REST API 和 OpenAPI | 契约测试覆盖版本、Grant、健康和敏感字段 |
| RES-010 | planned | RES-009、FND-011 | Credential 与全部资源列表/详情页面 | Mock 移除，加载、错误、空状态和权限一致 |
| RES-011 | planned | RES-007、RES-010 | 版本、依赖和授权界面 | 可解释显示某 Workflow 缺失的直接与间接权限 |

## 9. 失败、安全和幂等边界

- Credential 更新创建新 Secret Version，不返回旧值，也不允许读取后再回写。
- Connection Test 使用短期解析出的 Secret，结果只保存状态、耗时和脱敏错误。
- 删除被 Version Snapshot 引用的资源时转为停用，不物理删除历史数据。
- Grant 撤销立即影响新执行；正在运行的节点按阶段 10 的运行策略处理。
- Skill 代码能力不在 Platform API 执行，Manifest 不得声明绕过 CubeSandbox 的 Runner。

## 10. 测试

- Secret 加密、轮换、脱敏和主密钥错误测试。
- Price、Tool、Skill 版本不可变和 Hash 测试。
- Skill 依赖图、循环依赖和授权矩阵测试。
- LightRAG、Mem0 连接测试使用受控 Fake Server，不依赖公网。
- 前端连接测试、授权解释和敏感字段不可回显测试。
- 两租户资源 ID 猜测和 Grant 越权端到端测试。

## 11. 验收门禁

- Credential 明文不返回前端、不进入日志和 Trace。
- Model、Tool、Skill、RAG 和 Memory 控制面全部使用真实 API。
- Workflow 只能配置被授权资源，发布能固化资源版本和依赖快照。
- Skill 的 Tool、Model 和 Credential 依赖分别授权。
- 资源撤权不改写历史 Version，能够阻止后续新执行。

## 12. 对后续阶段的稳定输出

- Resource Reference、Version Snapshot 和 Grant API。
- Credential Resolver 与 Resource Authorizer。
- 模型、Tool、Skill、RAG 和 Memory 控制面对象。
- 运行 Adapter 所需的不可变配置和授权检查入口。
