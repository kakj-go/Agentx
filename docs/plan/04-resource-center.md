# 阶段 04：资源中心

## 1. 目标与用户价值

让租户集中接入并授权模型、MCP Server/Tool、在线 Skill Workspace、LightRAG、Mem0 和 Credential，确保设计时可选择、发布时可固化、运行时可二次校验。

## 2. 当前状态和进入条件

- 状态：`done`。旧 Tool 和 ZIP-only Skill 已废弃；MCP、Skill Workspace、统一资源授权和资源中心交互收口均已通过快速检查与 Kubernetes Playwright E2E。
- 进入条件：[阶段 03](03-workflow-control-plane.md) 的 Workflow、Service Identity、成员与 Grant 主体边界完成；Version 的最终资源快照和发布校验在本阶段资源模型稳定后共同收口。
- Credential、Model、MCP、Skill、Knowledge 和 Memory 页面必须接入真实 API，并提供连接测试或授权入口。
- M2 实际开发顺序、阶段交叉依赖和子任务见 [M2 实施任务清单](m2-task-list.md)。

## 3. 范围和不做内容

实现资源控制面、版本、连接健康、依赖和授权。不在本阶段执行模型或 Skill Runtime、RAG 查询或 Memory 写入；MCP 仅提供受控连接诊断和 Tool 调试，不创建 Workflow Execution。运行 Adapter 在阶段 10 接入。

## 4. 领域对象、状态和不变量

- Credential 是 Secret 或外部 Secret Reference，API 永不返回可恢复明文。
- Model Alias 是 Workflow 推荐引用，Model Deployment 保存 Provider、Endpoint 和 Credential Reference。
- Model Price Version 不可变，历史成本按调用发生时版本计算。
- MCP Server 配置可版本化；MCP Tool 只由发现流程创建，Tool Version 固化 Schema，Tool Policy 保存启停、调试、超时和副作用等级。
- Skill Workspace 可变且使用乐观锁 Revision；根 `SKILL.md` 使用必填的 `name`、`description` frontmatter，界面将描述与富文本正文分栏编辑；Skill Version 不可变并固化每个文件、引用、Content Hash、Artifact 和依赖。
- RAG 和 Memory 授权区分读写范围。
- Grant 可以授予 Department 或 Workflow；运行时最终使用 Workflow Service Identity 检查。MCP Tool 引用必须展开为 MCP Tool、所属 MCP Server 和 Server Credential 三项独立 Grant。

## 5. 数据和 Migration

主要表：

- credentials、credential_secret_versions
- model_providers、model_deployments、model_aliases、model_price_versions
- mcp_servers、mcp_server_versions、mcp_discovery_runs、mcp_tools、mcp_tool_versions、mcp_tool_policies
- skills、skill_workspace_entries、skill_file_revisions、skill_versions、skill_version_files、skill_file_references、skill_dependencies
- rag_connections、rag_resources
- memory_connections、memory_namespaces
- resource_health_checks、resource_grants、workflow_resource_grants、workflow_skill_grants

Secret 使用版本化密文或 Kubernetes/外部 Secret Reference；本地密文由平台主密钥保护，并保存算法版本、Nonce 和轮换信息。

## 6. REST API、Port 和事件

- `/api/v1/credentials`
- `/api/v1/models/providers|deployments|aliases|prices`
- /api/v1/mcp/servers、/discover、/tools、/policy 和 /debug-invoke
- /api/v1/skills、/workspace、/entries、/files、/versions 和 Workspace ZIP 导入导出
- `/api/v1/knowledge/connections|resources`
- `/api/v1/memory/connections|namespaces`
- `/api/v1/resources/{type}/{id}/grants`
- 统一 `POST .../test-connection`

定义 `CredentialResolver`、`ResourceAuthorizer` 和只做连接验证的控制面 Adapter；运行 Port 由阶段 10 扩展。

## 7. 后端和前端改动

- Platform API 增加 credentials、models、mcp、skills、knowledge、memory 和 grants 模块。
- Resource Authorizer 接收 Tenant、Workflow Service Identity、资源、版本和操作类型。
- 前端新增 Credential 管理入口和各资源详情、版本、连接测试及依赖界面；授权统一进入 `/resource-grants`，按资源类型 Tab 分栏展示，不分散在资源详情页。
- /models、/mcp、/skills、/knowledge 和 /memory 列表及详情使用真实 API。
- 所有资源选择器只返回当前用户可见且目标 Workflow 可用的资源。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| RES-001 | done | FND-005、IAM-005 | Credential Schema、加密、外部引用和轮换 | API 与日志均不暴露明文，旧版本可受控轮换 |
| RES-002 | done | RES-001、WCP-006 | Model Provider、Deployment、Alias 和 Price Version | Alias 可切换 Deployment，历史价格版本不可改 |
| RES-003 | done | RES-001 | MCP Server、自动发现 Tool Version、Policy 和受控调试 | Tool Schema 不能人工修改，Echo MCP 两种传输测试通过 |
| RES-004 | done | RES-003、FND-005 | Skill 在线 Workspace、文件 Revision、引用、Version 和 Artifact | 根 `SKILL.md` 描述与正文可视化编辑，Content Hash 可复现，缺失引用与循环依赖被拒绝 |
| RES-005 | done | RES-001 | LightRAG Connection、Resource 和读写范围 | Connection Test 不保存查询内容到日志 |
| RES-006 | done | RES-001 | Mem0 Connection、Namespace 和读写范围 | Namespace 跨租户引用被拒绝 |
| RES-007 | done | RES-002–006、WCP-006 | Department/Workflow Grant 和 Resource Authorizer | Skill Grant 不传递依赖资源权限 |
| RES-008 | done | RES-002–007 | 统一连接测试、健康状态和审计事件 | 新模型默认为未测试；手动测试、配置失效、超时和 Secret 脱敏均可验证 |
| RES-009 | done | RES-001–008 | Resource REST API 和 OpenAPI | 契约测试覆盖版本、Grant、健康和敏感字段 |
| RES-010 | done | RES-009、FND-011 | Credential 与全部资源列表/详情页面 | Mock 移除，加载、错误、空状态和权限一致 |
| RES-011 | done | RES-007、RES-010 | 统一资源授权、版本和依赖界面 | 可集中授权 Department/Workflow，并解释 MCP Tool、Server、Credential 等直接与间接缺口 |

## 9. 失败、安全和幂等边界

- Credential 名称或状态修改只增加实体 Version；只有显式轮换 Secret 才创建 Secret Version，且不返回旧值。
- Connection Test 使用短期解析出的 Secret，结果只保存状态、耗时和脱敏错误。
- 删除被 Version Snapshot 引用的资源时转为停用，不物理删除历史数据。
- Grant 撤销立即影响新执行；正在运行的节点按阶段 10 的运行策略处理。
- Skill 代码能力不在 Platform API 执行，Manifest 不得声明绕过 CubeSandbox 的 Runner。

## 10. 测试

- Secret 加密、轮换、脱敏和主密钥错误测试。
- Price、Model Deployment Revision、MCP Tool Version、Skill Version 不可变和 Hash 测试。
- Skill 依赖图、循环依赖和授权矩阵测试。
- LightRAG、Mem0 连接测试使用受控 Fake Server，不依赖公网。
- 前端连接测试、授权解释和敏感字段不可回显测试。
- 两租户资源 ID 猜测和 Grant 越权端到端测试。

## 11. 验收门禁

- Credential 明文不返回前端、不进入日志和 Trace。
- Model、MCP、Skill、RAG 和 Memory 控制面全部使用真实 API。
- Workflow 只能配置被授权资源，发布能固化资源版本和依赖快照。
- Skill 的 MCP Tool、Model 和 Credential 直接及递归依赖分别授权。
- 资源撤权不改写历史 Version，能够阻止后续新执行。

## 12. 对后续阶段的稳定输出

- Resource Reference、Version Snapshot 和 Grant API。
- Credential Resolver 与 Resource Authorizer。
- 模型、MCP Server/Tool、Skill Workspace、RAG 和 Memory 控制面对象。
- 运行 Adapter 所需的不可变配置和授权检查入口。
