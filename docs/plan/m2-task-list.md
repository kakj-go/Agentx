# M2 实施任务清单：Workflow 控制面与资源中心

本文将[阶段 03](03-workflow-control-plane.md)和[阶段 04](04-resource-center.md)拆成可直接实施的批次。阶段文档定义领域边界，本清单定义实际开发顺序、任务依赖、接口冻结点和验收证据。

## 1. 目标与完成结果

M2 完成后，Company Admin 和获得授权的部门用户能够：

1. 创建、查看、编辑和归档 Workflow，并管理成员与可见范围。
2. 保存带 Revision 的最小 Workflow Draft，处理乐观锁冲突。
3. 管理 Credential、Model、Tool、Skill、LightRAG 和 Mem0 控制面资源。
4. 将直接资源及 Skill 的间接依赖显式授权给 Workflow Service Identity。
5. 从合法 Draft 创建不可变 Workflow Version，并固化资源版本快照。
6. 将明确 Version 发布到 Environment，查看历史并回滚 Deployment。
7. 在真实页面完成上述操作，不再使用对应 Feature 的 Mock 数据。

M2 不产生真实 Execution。运行、测试执行、Agent 调用和节点调试入口必须返回 `RUNTIME_UNAVAILABLE` 或保持禁用，并明确说明运行引擎将在后续里程碑接入。

## 2. 当前基线与进入条件

- M1 已完成，阶段 01、02 及 FND-001～FND-011、IAM-001～IAM-011 均为 `done`。
- 单公司 Bootstrap、JWT、Refresh Token、部门树、用户、角色、数据范围和审计能力可复用。
- MySQL Migration 当前到 `0003_m1_hardening.sql`，已发布 Migration 不得修改。
- OpenAPI 导出、TypeScript Client、同源 `/api/v1`、统一错误和 Request ID 已可用。
- Artifact、Outbox、事务和依赖健康检查基础 Port 已存在。
- `/workflows`、`/models`、`/tools`、`/skills`、`/knowledge` 和 `/memory` 当前仍为 Mock 页面。

开始 M2 编码时，将本文件、阶段 03、阶段 04 和总计划中的 M2 状态改为 `in_progress`；仅完成规划不改变状态。

## 3. 范围与明确不做内容

### 3.1 本阶段范围

- Workflow 元数据、成员、Service Identity、Draft、Revision、Version、Environment、Deployment、发布和回滚。
- Credential 安全存储或外部 Secret Reference、轮换元数据和脱敏展示。
- Model Provider、Deployment、Alias 和不可变 Price Version。
- Tool Definition 和不可变 Tool Version。
- Skill Definition、不可变 Skill Version、Artifact、Manifest 和依赖图。
- LightRAG Connection/Resource 与 Mem0 Connection/Namespace。
- Department Grant、Workflow Grant、资源选择和发布时授权解释。
- 真实 REST API、OpenAPI、TypeScript Client、前端页面、审计和自动化测试。

### 3.2 本阶段不做

- Workflow Scheduler、Execution、Node Execution、表达式执行和 Redis 任务调度。
- 模型推理、Tool 调用、Skill 加载、RAG 查询、Memory 读写和 CubeSandbox。
- 完整 React Flow Studio、节点面板、连线校验、Pin Data 和局部执行。
- n8n JSON 导入、连接器市场、OIDC 登录、通用监控和计费。
- 物理删除已被历史 Version 引用的 Workflow 或资源。

`/workflows/:workflowId/editor` 在 M2 只承担最小 Definition JSON 或已有示例画布到 Draft DTO 的保存验证，不将 React Flow State 定义为运行协议，也不宣称具备执行能力。

## 4. 核心不变量

- 所有表、Repository 和查询继续携带 `tenant_id`，客户端不能指定或覆盖 Tenant Context。
- Workflow 创建时在同一事务创建唯一 Service Identity；生产授权不继承设计者个人权限。
- Draft 可变且通过 `revision` 乐观锁保存；Draft Revision 只追加、不覆盖。
- Workflow Version、Tool Version、Skill Version 和 Model Price Version 一经创建不可修改。
- Canonical JSON Hash 不受对象字段顺序影响；相同 Revision 和 Hash 的重试不得生成重复 Version。
- Deployment 永远指向明确 Workflow Version；同一 Workflow 和 Environment 只有一个 Active Deployment。
- Credential 明文不得通过读取 API、日志、审计、错误、OpenAPI 示例或前端状态回显。
- Model Alias 可以改变目标 Deployment，但 Workflow Version 必须保存发布时解析快照。
- Skill Grant 不隐式授予其 Tool、Model、Credential、RAG 或 Memory 依赖权限。
- 资源撤权不改写历史 Version；新发布立即失败，未来新执行还必须进行运行时二次校验。
- 连接测试只验证控制面连通性，不执行用户 Workflow，也不持久化业务请求或响应正文。

## 5. 依赖与实施批次

```text
M2-0 契约、权限与 Migration
  └─ M2-1 Workflow 容器、成员和 Service Identity
       └─ M2-2 Draft、Revision 和 Definition
            ├─ M2-3 Credential 安全基础
            ├─ M2-4 Model 与 Tool
            ├─ M2-5 Skill、Artifact 与依赖
            └─ M2-6 LightRAG 与 Mem0
                    ↓
              M2-7 统一 Grant 与授权解释
                    ↓
              M2-8 Version、发布与回滚
                    ↓
              M2-9 真实前端页面收口
                    ↓
              M2-10 集成、安全与发布验收
```

阶段 04 不等待阶段 03 全部结束才启动。完成 Workflow、Service Identity 和 Grant 主体边界后即可并行建设资源；Workflow 发布校验在资源版本与授权稳定后收口。

## 6. 详细任务

状态只使用 `planned`、`in_progress`、`blocked` 和 `done`。下表中的 M2 编号按批次分段，未使用的尾号是预留编号，不代表遗漏任务；`映射`保留原阶段任务的唯一归属。

### 6.1 M2-0：契约、权限与数据基础

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-001 | planned | WCP-003、RES-007 | M1 | Workflow/Resource 公共 ID、Reference、Grant、Version Snapshot 和枚举 Schema 评审记录 | Domain、API DTO 和前端生成类型职责明确，无数据库实体直接暴露 |
| M2-002 | planned | WCP-006、RES-007 | M2-001 | M2 权限种子、内置角色增量和权限矩阵 | Company Admin、Department Admin、Member 与自定义角色用例覆盖允许和拒绝路径 |
| M2-003 | planned | WCP-001–006 | M2-001 | `0004_workflow_control_plane.sql` | 空库和 M1 已有库均可向前迁移，唯一键和 tenant_id 约束通过集成测试 |
| M2-004 | planned | RES-001–007 | M2-001 | `0005_credentials_and_models.sql`、`0006_tools_and_skills.sql`、`0007_rag_memory_and_resource_grants.sql` | Migration 可重复检查且不会修改 `0001`～`0003` |
| M2-005 | planned | WCP-008、RES-009 | M2-001–004 | Repository、事务边界、审计与 Outbox 事件约定 | 两个测试 tenant_id 的 ID 猜测和列表越权均返回不可见或拒绝 |

### 6.2 M2-1：Workflow 容器、成员和运行身份

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-010 | planned | WCP-001 | M2-003、M2-005 | Workflow 聚合、状态、Repository 和 Service Identity | 创建 Workflow 与 Service Identity 同事务提交，失败不留下孤立记录 |
| M2-011 | planned | WCP-001、WCP-008 | M2-010 | Workflow 创建、读取、更新、归档 API 与审计事件 | 归档后禁止修改 Draft 和发布，历史引用仍可读取 |
| M2-012 | planned | WCP-006 | M2-002、M2-010 | Workflow Member、角色和可见范围 | 非成员且无组织数据范围的用户不能列出或读取目标 Workflow |
| M2-013 | planned | WCP-006、WCP-010 | M2-012 | Workflow 成员管理和作用域 API | 授权人不能授予自身不具备的权限，所有变更写入审计 |

### 6.3 M2-2：Draft、Revision 与 Definition

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-020 | planned | WCP-002 | M2-010 | Draft 和 append-only Draft Revision Repository | 使用旧 Revision 保存返回 `409`，服务器内容和冲突草稿均不丢失 |
| M2-021 | planned | WCP-003 | M2-001、M2-020 | 最小 Workflow Definition JSON Schema、Schema Version 和 Validator | 非法节点 ID、重复 ID、悬空边、未知顶层字段按契约拒绝 |
| M2-022 | planned | WCP-003 | M2-021 | Canonical JSON、Content Hash 和 Definition 升级接口边界 | 对象字段顺序不影响 Hash，数组顺序仍保持业务语义 |
| M2-023 | planned | WCP-002、WCP-008 | M2-020–022 | Draft 读取、保存、Revision 历史 API 和 OpenAPI | 契约测试覆盖首次保存、幂等重试、冲突和历史读取 |

### 6.4 M2-3：Credential 安全基础

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-030 | planned | RES-001 | M2-004–005 | Credential 类型、Secret Envelope、主密钥配置和脱敏类型 | 缺失或错误主密钥时服务明确失败或资源功能不可用，不输出密文和明文 |
| M2-031 | planned | RES-001 | M2-030 | 本地加密 Secret Version 与外部 Secret Reference | 更新产生新版本，读取只返回掩码、类型、版本和更新时间 |
| M2-032 | planned | RES-001、RES-009 | M2-002、M2-031 | Credential CRUD、轮换、停用 API、权限和审计 | 创建响应也不返回可恢复明文；日志、错误和审计通过敏感信息扫描 |
| M2-033 | planned | RES-008 | M2-031 | Credential Resolver 控制面接口和短生命周期 Secret 容器 | Secret 使用后清理，Debug/Serialize 均不能输出内容 |

### 6.5 M2-4：Model 与 Tool 控制面

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-040 | planned | RES-002 | M2-032 | Model Provider、Deployment、Alias、Price Version 领域与 Repository | Alias 可原子切换 Deployment，历史 Price Version 不可更新 |
| M2-041 | planned | RES-002、RES-009 | M2-040 | Model REST API、分页筛选、权限和审计 | API 不展开 Credential Secret，跨部门不可见模型不能被引用 |
| M2-042 | planned | RES-003 | M2-032 | Tool Definition、Tool Version、输入输出 Schema、副作用等级、超时和 Runner 类型 | 已发布 Tool Version 不可变，无效 JSON Schema 被拒绝 |
| M2-043 | planned | RES-003、RES-009 | M2-042 | Tool REST API、版本创建、停用和审计 | 重复 Content Hash 幂等，历史版本仍可被快照读取 |
| M2-044 | planned | RES-008 | M2-033、M2-040、M2-042 | 可取消的 Model/HTTP Tool 控制面连接测试 Adapter | 超时和错误被脱敏，测试不创建 Execution 或 Trace |

### 6.6 M2-5：Skill、Artifact 与依赖图

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-050 | planned | RES-004 | M2-004、M2-042 | Skill Manifest Schema、Definition、Version 和 Content Hash | Manifest 声明的资源类型受白名单约束，不能声明 Worker 内直接执行高风险代码 |
| M2-051 | planned | RES-004 | M2-050 | Skill Artifact 上传、Hash、对象存储元数据和失败清理 | Artifact Hash 与元数据一致，事务失败不会留下可引用孤儿对象 |
| M2-052 | planned | RES-004 | M2-040–043、M2-050 | Skill 依赖图和循环检测 | 直接循环、间接循环、跨租户依赖和不存在版本均被拒绝 |
| M2-053 | planned | RES-004、RES-009 | M2-051–052 | Skill REST API、版本、依赖、Artifact 下载授权和审计 | 下载使用授权后的短期访问方式，未授权用户不能根据 Object Key 读取 |

### 6.7 M2-6：LightRAG 与 Mem0 控制面

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-060 | planned | RES-005 | M2-032–033 | LightRAG Connection、Knowledge Resource、读范围和同步状态 | Connection 与 Resource 均受 tenant_id 和部门范围约束 |
| M2-061 | planned | RES-005、RES-008–009 | M2-060 | LightRAG REST API 和 Fake Server 连接测试 | 请求/响应正文不写日志，超时可取消且错误脱敏 |
| M2-062 | planned | RES-006 | M2-032–033 | Mem0 Connection、Namespace、读写权限和状态 | Namespace 唯一性和跨租户引用约束通过测试 |
| M2-063 | planned | RES-006、RES-008–009 | M2-062 | Mem0 REST API 和 Fake Server 连接测试 | 本阶段不创建真实 Memory 记录，失败不暴露 Secret |

### 6.8 M2-7：统一资源授权与发布解释

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-070 | planned | RES-007 | M2-012、M2-040～044、M2-050～053、M2-060～063 | 统一 Resource Type/Operation、Department Grant 和 Workflow Grant | 同一 Authorizer 覆盖 Model、Tool、Skill、RAG、Memory 和 Credential |
| M2-071 | planned | RES-007 | M2-070 | Resource Authorizer 与 Workflow Service Identity 授权解析 | 用户可管理资源不等于 Workflow 可运行资源，两条权限链分别校验 |
| M2-072 | planned | RES-007、RES-011 | M2-052、M2-071 | Skill 直接/间接依赖展开和缺失授权解释 | 只授权 Skill 时，未授权 Tool/Model/Credential 被逐项报告而非隐式放行 |
| M2-073 | planned | WCP-006、RES-009 | M2-070–072 | Grant CRUD、批量校验、可选资源查询 API 与审计 | Department Admin 不能向范围外 Workflow 或资源授权，重复请求幂等 |
| M2-074 | planned | RES-008 | M2-044、M2-061、M2-063 | 统一资源健康结果、最近检查和状态更新规则 | 连接测试并发受限，旧响应不能覆盖较新的检查结果 |

### 6.9 M2-8：Version、Environment、发布与回滚

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-080 | planned | WCP-004、WCP-007 | M2-022–023、M2-070–073 | Publish Validator 和不可变 Resource Version Snapshot | Schema、资源不存在、停用、缺 Grant 或间接依赖缺失均阻止创建 Version |
| M2-081 | planned | WCP-004 | M2-080 | Workflow Version Repository、顺序号和幂等创建 API | 相同 Draft Revision 与 Hash 的重试返回同一 Version，历史内容无更新 API |
| M2-082 | planned | WCP-005 | M2-003、M2-081 | Environment 种子、Deployment 状态机和 History | 同一 Workflow/Environment 仅一个 Active，状态迁移通过数据库约束和事务保证 |
| M2-083 | planned | WCP-005、WCP-008 | M2-082 | 发布、回滚、历史查询 API、审计和 Outbox 事件 | 并发发布只有一个目标成为 Active，回滚创建新历史记录且不修改旧 Version |
| M2-084 | planned | WCP-007、RES-007 | M2-080–083 | 撤权、停用、归档与发布策略集成 | 历史快照可读，新发布被阻止；所有失败返回结构化原因和 Request ID |

### 6.10 M2-9：真实前端页面收口

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-090 | planned | WCP-009–010 | M2-011–013、M2-023、M2-081–084 | Workflow 列表、创建、详情、成员、Draft、Version、Deployment 和 Grant 页面 | `/workflows` 不再读取 Mock；冲突、权限不足、发布失败和空状态可见 |
| M2-091 | planned | RES-010–011 | M2-032–033 | Credential 列表、创建、轮换、停用和授权页面及菜单入口 | 页面从不回显 Secret，复制和浏览器状态中无明文 |
| M2-092 | planned | RES-010–011 | M2-041、M2-043–044 | Model、Tool 列表/详情、版本、价格、连接测试和授权页面 | 搜索、筛选、分页、错误、权限和中英文状态统一 |
| M2-093 | planned | RES-010–011 | M2-053、M2-072–074 | Skill 列表/详情、版本、Artifact、依赖图和授权解释页面 | 可明确显示每个间接依赖及授权缺口，不使用含糊的“不可用”提示 |
| M2-094 | planned | RES-010–011 | M2-061、M2-063、M2-073–074 | Knowledge 与 Memory 列表/详情、连接、范围、健康和授权页面 | Mock 全部移除，连接测试结果不显示敏感请求或响应 |
| M2-095 | planned | WCP-009、RES-010 | M2-090–094 | 统一资源选择器、权限指令和未实现运行提示 | 选择器仅显示对当前用户可见且可授权给目标 Workflow 的资源 |

前端继续复用 `shared/ui`、`shared/components`、Tailwind 语义 Token、Radix、TanStack Query/Table 和 i18next。Feature 禁止直接调用 `fetch`，不引入第二套 UI 库，单文件不得超过 2000 行。

### 6.11 M2-10：集成、安全与发布验收

| 编号 | 状态 | 映射 | 依赖 | 交付物 | 可验证验收条件 |
|---|---|---|---|---|---|
| M2-100 | planned | WCP-008、RES-009 | M2-011、M2-013、M2-023、M2-032、M2-041、M2-043、M2-053、M2-061、M2-063、M2-073～074、M2-081、M2-083～084 | OpenAPI 导出、生成 Client 和漂移检查 | 后端重新生成 OpenAPI 与仓库文件无差异，前端类型检查通过 |
| M2-101 | planned | WCP-001–010 | M2-090、M2-100 | Workflow 单元、MySQL 集成、API 契约和前端测试 | Revision、Hash、不可变 Version、并发发布、回滚、归档和权限矩阵通过 |
| M2-102 | planned | RES-001–011 | M2-091～095、M2-100 | Resource 单元、Fake Server、MinIO、MySQL、API 和前端测试 | Secret、版本、依赖、Grant、连接超时、跨租户和 Artifact 权限通过 |
| M2-103 | planned | WCP-007、RES-007 | M2-101–102 | M2 端到端业务闭环和越权测试 | 从创建 Workflow 到资源授权、Version、发布、撤权阻断和回滚全程使用真实 API |
| M2-104 | planned | WCP-008、RES-008 | M2-103 | Kubernetes 配置、Secret/主密钥、Migration Job 和滚动升级验证 | M1 数据升级后服务 Ready，多副本发布/轮换无进程内状态依赖 |
| M2-105 | planned | WCP-001–010、RES-001–011 | M2-100–104 | M2 验收证据、阶段状态和追踪矩阵更新 | 全部门禁通过后阶段 03、04 和 M2 同步标记 `done` |

## 7. 稳定公共契约与冻结点

### 7.1 M2-0 后冻结

- `WorkflowId`、`WorkflowVersionId`、`EnvironmentId`、`DeploymentId`。
- `ResourceId`、`ResourceVersionId`、`ResourceType`、`ResourceOperation`。
- `WorkflowServiceIdentity`、`ResourceReference`、`ResourceGrant`、`ResourceVersionSnapshot`。
- M2 权限 Key、分页、乐观锁和错误码命名。

### 7.2 M2-2 后冻结

- `WorkflowDefinition` 最小 JSON Schema、`schemaVersion` 和升级边界。
- Draft Revision 协议、Canonical JSON 和 Content Hash 算法版本。
- Draft 保存的 `If-Match` 或等价 Revision 前置条件。

### 7.3 M2-7 后冻结

- `CredentialResolver`：仅在服务端返回短生命周期 Secret 容器。
- `ResourceAuthorizer`：接收 Tenant、Actor 或 Workflow Service Identity、资源版本和操作。
- 资源依赖展开与 `MissingGrant` 结构化解释。
- 连接测试统一请求、状态、超时和脱敏错误格式。

### 7.4 M2-8 后冻结

- Workflow Version 创建、资源快照、发布、回滚和 Deployment History API。
- `WorkflowVersionCreated`、`WorkflowPublished`、`WorkflowRolledBack` 和资源变更 Outbox Event。
- 为 M3 提供稳定的 Workflow/Version/Deployment 引用，不再依赖 Draft。

具体字段在对应任务开始前完成 Schema 评审，评审结果直接回写阶段文档或 OpenAPI，不另建长期未决清单。

## 8. 权限种子与内置角色增量

M2 至少追加以下权限：

| 领域 | 权限 |
|---|---|
| Workflow | `workflow:view`、`workflow:create`、`workflow:edit`、`workflow:archive`、`workflow:publish`、`workflow:manage_member`、`workflow:manage_permission` |
| Credential | `credential:view`、`credential:manage` |
| Model | `model:view`、`model:manage` |
| Tool | `tool:view`、`tool:manage` |
| Skill | `skill:view`、`skill:manage` |
| Knowledge | `knowledge:view`、`knowledge:manage` |
| Memory | `memory:view`、`memory:manage` |
| Grant | `resource:grant` |

默认策略：

- `company_admin` 获得全部 M2 权限。
- `department_admin` 默认可以查看本部门树可见资源；管理、发布和 Grant 权限必须由 Company Admin 显式配置，且作用域不超过其部门树。
- `member` 默认不获得管理权限，只能通过自定义角色、Workflow Member 或资源可见范围获得最小访问。
- 自定义角色仍执行“目标权限必须是操作者有效权限子集”的 M1 防提权规则。
- 前端隐藏无权限操作，但后端始终独立进行 Permission、Data Scope 和 Resource Grant 校验。

最终角色增量应由 M2-002 的权限矩阵评审确定，并同步更新阶段 03、04 的测试用例。

## 9. Migration 顺序

只允许向前追加：

1. `0004_workflow_control_plane.sql`
   - workflows、workflow_members、workflow_service_identities
   - workflow_drafts、workflow_draft_revisions
   - workflow_versions、workflow_version_resources
   - environments、workflow_deployments、deployment_history
2. `0005_credentials_and_models.sql`
   - credentials、credential_secret_versions
   - model_providers、model_deployments、model_aliases、model_price_versions
3. `0006_tools_and_skills.sql`
   - tools、tool_versions
   - skills、skill_versions、skill_dependencies 和 Artifact 引用
4. `0007_rag_memory_and_resource_grants.sql`
   - rag_connections、rag_resources
   - memory_connections、memory_namespaces
   - resource_grants、workflow_resource_grants、resource_health_checks
   - M2 Permission 与内置角色增量

表名可以在 M2-001 Schema 评审时收敛，但 Migration 边界、不可变历史和向前追加规则不得改变。发布前必须验证从空库和真实 M1 Schema 两条路径升级。

## 10. API 与错误约定

除阶段 03、04 已列出的资源外，M2 统一遵循：

- 列表使用 `page/pageSize`，支持明确白名单字段的搜索、状态和所有者筛选。
- 写操作使用 Revision、Version 或状态条件，冲突返回 `409` 和稳定错误码。
- 创建 Version、发布、回滚、Grant 和连接测试支持 `Idempotency-Key` 或等价幂等键。
- 权限不足返回 `403`；为避免 ID 枚举，跨 tenant_id 或完全不可见对象可返回 `404`。
- 运行相关调用统一返回 `503 RUNTIME_UNAVAILABLE`，不能创建占位 Execution。
- 发布校验返回结构化问题列表，至少区分 Schema、资源不存在、版本停用、缺少直接 Grant 和缺少间接依赖 Grant。
- 所有错误保持 M1 的 `code`、`message`、`requestId` 和可选 `fieldErrors` 格式。

## 11. 测试矩阵

| 层级 | 必须覆盖 |
|---|---|
| 单元测试 | Canonical JSON、Hash、状态机、版本不可变、Secret 加密/脱敏、Schema、Skill 依赖图、授权矩阵 |
| MySQL 集成 | Migration、事务回滚、Revision 冲突、并发发布、环境唯一 Active、tenant_id 防护、Grant 撤销 |
| MinIO 集成 | Skill Artifact Hash、失败清理、授权下载、历史引用 |
| Fake Server | Model/Tool/LightRAG/Mem0 连接成功、超时、取消、脱敏错误和恢复 |
| API 契约 | 分页、错误、幂等、乐观锁、敏感字段、OpenAPI 漂移和生成 Client |
| 前端 | 路由权限、列表/详情、表单校验、冲突、发布解释、连接测试、空状态、中英文和浅深主题 |
| 安全 | 跨 tenant_id ID 猜测、部门树越权、角色提权、Credential 泄漏扫描、Artifact Object Key 绕过 |
| Kubernetes | M1 Schema 升级、Migration Job、多副本、主密钥配置、MySQL/MinIO 中断和恢复 |
| 端到端 | Workflow → Draft → 资源 → Grant → Version → Deployment → 撤权阻断新发布 → 回滚 |

Testcontainers 或受控容器负责自动集成测试，测试不得依赖开发者已有 Kubernetes 数据或公网服务。

## 12. Kubernetes 与配置变化

- Platform API 增加 Credential 主密钥、连接测试超时、并发限制和 Artifact 上传限制配置。
- 主密钥只能来自 Kubernetes Secret 或外部 Secret 注入，不能进入 ConfigMap、镜像和 Git。
- M2 继续使用 Platform API 模块化单体，不新增 Worker、Coordinator 或 Sandbox Deployment。
- Migration Job 使用同一 Platform API 镜像执行 `migrate`，并覆盖 `0004`～`0007`。
- Readiness 仍以 MySQL 为必需依赖；MinIO 在 Skill Artifact 用例中不可用时，相关接口返回明确依赖错误，不伪造成功。
- LightRAG、Mem0 和 Model Provider 是用户配置的外部连接，不作为 Platform API 全局 Readiness 的必需依赖。

## 13. M2 验收门禁

只有以下条件全部成立才可将 M2 标记为 `done`：

1. WCP-001～WCP-010、RES-001～RES-011 和本文列出的 52 项 M2 执行任务全部为 `done`。
2. 对应 Workflow 与资源页面全部使用真实 API，Mock 数据已从这些 Feature 移除。
3. Draft 冲突可恢复，Workflow/Tool/Skill/Price Version 不可变，Deployment 可发布和回滚。
4. Credential 在 API、前端、日志、错误、审计和测试快照中均无明文泄漏。
5. Workflow 只能选择和发布已授权的直接资源，Skill 间接依赖逐项授权并可解释。
6. LightRAG 和 Mem0 完成控制面连接、范围和授权，但没有伪造运行结果。
7. 运行入口明确不可用且没有创建 Execution、Trace 或 Evaluation 假记录。
8. Rust fmt、Clippy、单元/集成测试、Oxlint、TypeScript、Vite、OpenAPI 漂移、Kustomize 和 `git diff --check` 全部通过。
9. 全新数据库与 M1 数据库升级、Kubernetes 多副本和依赖故障场景均有可复现证据。
10. `README.md`、阶段 03/04、路线图、追踪矩阵和 M2 验收证据状态同步。

## 14. 向 M3 提供的稳定输出

- 可引用的 Workflow、Workflow Version、Environment 和 Deployment。
- Workflow Service Identity、Workflow Member 和 Resource Grant 授权入口。
- 不可变 Workflow Definition、Resource Version Snapshot 和发布历史。
- Credential Resolver 和 Resource Authorizer 控制面 Port。
- Model Alias/Deployment、Tool Version、Skill Version、RAG Resource 和 Memory Namespace。
- M3 可使用的资源选择、版本引用、权限检查、审计和 Outbox Event。
- 对尚未接入运行时的所有操作提供统一 `RUNTIME_UNAVAILABLE` 契约。
