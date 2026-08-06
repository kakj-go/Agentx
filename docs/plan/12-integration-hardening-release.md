# 阶段 12：全链路集成、生产加固和首期发布（M7）

## 1. 目标与完成边界

M7 将 M1～M6 已完成的控制面、Runtime、Agent/OpenSandbox 和 Studio 接成一个可发布产品，并在生产候选 Kubernetes 集群完成租户隔离、配额、Credential、供应链、故障、容量、升级和十二步业务闭环验收。

M7 不再建设新的画布能力、节点表单或调试模式。进入 M7 时若 M6 的 Studio 仍依赖 Mock、隐藏 Version 或 `RUNTIME_UNAVAILABLE`，应退回 M6 修复，不能在 M7 复制一套旁路。

M7 完成代表 Agentx 首期 MVP/内部生产版基本完成，不代表通用商业平台全部完成。n8n 兼容、连接器市场、实时协作、移动端、OIDC、商业计费、跨区域多活和通用监控仍不属于 M7。

## 2. 进入条件

- 阶段 01～11 全部为 `done`，M6 的 STU-001～016 有独立验收证据。
- Definition 3.0、Node Catalog、Version/Draft Revision Execution Source、Runtime Event 和错误码已冻结。
- Studio 已能通过 Draft Revision Snapshot 完成 Full/Single/To/From 调试、Trace、Version 和发布。
- M7 开始时建立所有 `RUNTIME_UNAVAILABLE`、Fake Adapter、未消费 Outbox、孤立页面和业务 Mock 的清单，逐项映射到 INT-001～014；清单外不得临时扩大产品范围。
- AGT-010 的生产强化保持“暂不在 M5 重试”，由 INT-006/010/011/014 唯一承接。

## 3. 最终架构

### 3.1 单一 Execution 主链

```text
Studio Draft Debug ------------------ Platform API --+
Application / Playground / API ------ Trigger Gateway +--> ExecutionRuntime
Webhook / Schedule / Poll ----------- Trigger Gateway +--> Coordinator
Evaluation -------------------------- Platform API --+        |
Approval / External Resume ---------- Trigger Gateway --------+
                                                              v
                                          Execution Snapshot -> Worker
                                                              |
                                          MySQL Outbox -> Runtime Events
                                                              |
                           Session / Evaluation / Approval / Notification / SSE Projections
```

- Studio Draft Debug 使用 M6 的 `DraftRevision` Source；其他生产入口只使用不可变 `Version` Source。
- 一个业务请求只能产生一条权威 Invocation/Execution 链；外围对象保存稳定 `execution_id`，不得生成伪 Execution 或二次旁路记录。
- Coordinator 是 Execution 状态推进的唯一写入边界。Application、Evaluation、Approval 和 Notification 通过 Port/Command 和 Outbox Event 接入，不直接修改运行表。
- SSE 只是可恢复投影。客户端用 Cursor 重连并以 MySQL 查询校准，不把 Redis 或长连接当权威状态。

### 3.2 事件和投影

冻结版本化 Runtime Event Envelope：`eventId/tenantId/eventType/schemaVersion/aggregateType/aggregateId/executionId/occurredAt/payload`。Outbox 至少一次投递，所有消费者以 `tenant_id + event_id + projector` 幂等。

M7 只建设现有领域投影：

- Invocation/Message 状态和 SSE 回放。
- Approval Task 创建、决策和恢复。
- Evaluation Case Result、指标和报告。
- Notification 和业务跳转。
- Runtime Status、成本和配额使用量。

不得新增通用 Event Bus 产品或 CQRS 服务；继续使用 MySQL Outbox、Redis Stream 和现有服务边界。

### 3.3 运行授权和配额

执行创建时固化资源版本，但每次新 Execution 仍检查 Workflow Service Identity、Resource Grant 和资源状态。执行中已取得的短期 Handle 按 Lease/Deadline 完成或失效；撤权不改写历史 Snapshot，也不能被后续重试绕过。

配额采用“权威限额 + 可回收预留”：

- MySQL 保存租户限额、Reservation、Usage Ledger 和最终结算。
- Redis 只做 admission 快速计数与限流，重建后可从 MySQL 校准。
- Coordinator 在创建/派发前预留 Execution、Node、Agent、Token/Cost 预算；Worker/Sandbox Manager 在实际使用前检查并增量结算。
- Lease 过期、取消、Worker 崩溃和 Sandbox 回收必须释放 Reservation；重复事件不能重复扣减。
- 限额覆盖并发 Execution/Node/Sandbox、单次时长、Agent Iteration、Token、Cost、Artifact 大小、CPU、内存、PID、临时磁盘和 TTL。

### 3.4 Credential 与 Sandbox 生产边界

- 保留 `SecretProvider` Port；生产实现使用 Vault 或经过安全评审的等价服务，数据库只保存 Secret Reference、版本和非敏感元数据。
- Worker 不读取长期明文；一次性 Credential Handle 继续绑定 Tenant、Execution Snapshot、Node Attempt、Lease 和 Deadline。
- Sandbox Manager 是 OpenSandbox API Key、execd Token 和凭证文件注入的唯一持有者；销毁 Sandbox 或释放 Lease 后 Handle 立即失效。
- Sandbox Profile 只能引用批准的 RuntimeClass、镜像 Digest、资源上限和 Egress Policy；用户不能提交任意 RuntimeClass、镜像 Tag 或宿主路径。
- 生产 Pod 关闭 ServiceAccount Token，使用 non-root、read-only rootfs、capability drop、seccomp/AppArmor、PID/CPU/内存/临时磁盘限制和默认拒绝双栈 Egress。
- Release Candidate 镜像固定 Digest、生成 SBOM、签名并在部署/准入阶段校验；浮动 Tag 不能进入生产 Overlay。

### 3.5 发布后的兼容边界

项目当前尚未发布，M6 的 Definition 3.0 和 Execution Source 改造可一次性清理开发数据，不维护历史双读。M7 生成首个 Release Candidate 后开始执行正式兼容规则：

- Workflow Version、Execution Snapshot、Dataset Version 和资源版本不可重写。
- Migration 支持 expand/migrate/contract 和滚动部署；已执行 Migration 不随应用回滚。
- 新 Worker 通过 Capability/Compiler Version 领取任务；旧 Worker 不领取未知 Manifest/IR。
- 外部 OpenAPI、内部 gRPC、Runtime Event 和 Node Protocol 破坏性变化必须升版本。

## 4. M7 范围分组

### M7-0：外围入口贯通

- Application、Session、Message、Playground/API 和 SSE 到真实 Execution。
- Approval Node、Task、Notification、Decision 和 Resume。
- Evaluation Case 到真实 Execution、评分和报告。
- Webhook、Schedule、Poll、activate/deactivate 与 Version Deployment。

### M7-1：运行治理

- 运行时二次授权与撤权。
- 租户配额、限流、Reservation 和结算。
- Runtime Event 到通知和外围对象投影。
- Trace/Artifact/Message/Report 保留和引用安全清理。

### M7-2：生产安全与可靠性

- 滚动 Migration、升级和回滚演练。
- RuntimeClass、资源强制、双栈 Egress 和依赖故障。
- Vault、Secret、API Key、签名镜像和跨租户攻击矩阵。

### M7-3：发布门禁

- 容量、水平扩容和长时间稳定性。
- 正式页面、国际化、主题和无障碍收口。
- 全新集群安装、十二步 MVP、持久化证据和发布清单。

### 4.1 服务责任收口

| 模块 | M7 最终责任 |
|---|---|
| `trigger-gateway` | Application/API/Webhook/Schedule/Poll/Resume 的认证、幂等、Version 解析和 SSE，不保存第二套 Execution 状态 |
| `platform-api` | Application/Evaluation/Approval/Notification 投影、运行授权、Catalog/Version 查询和管理 API |
| `workflow-coordinator` | Execution admission、配额 Reservation、状态推进、取消/恢复、Outbox 和故障回收 |
| `workflow-worker` | Attempt Lease、资源二次授权、预算结算、Node Runner 和可解释失败 |
| `sandbox-manager` | RuntimeClass/Profile/Digest 校验、Sandbox 生命周期、资源强制、短期凭证和残留回收 |
| `trace-writer` | Trace 至少一次消费、批量写入、脱敏和 ClickHouse 恢复补投 |
| `web-console` | 只消费真实 API；M7 修复外围页面，不改变 M6 Studio 契约和交互 |
| `deploy/kustomize/scripts` | Vault、RuntimeClass、NetworkPolicy、签名镜像、Migration、扩容和分阶段证据 |

## 5. 实施任务

| 编号 | 批次 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|---|
| INT-001 | M7-0 | done | APP-005–011、RUN-014、STU-016 | Application Invocation、Session、Message、Playground/API、SSE 到真实 Execution | Invocation/Message/Execution/Trace 双向定位；幂等重放不重复运行；SSE Cursor 可恢复 |
| INT-002 | M7-0 | done | OBS-001–003、REC-005–006、STU-016 | Approval Node、Task、Notification、Decision、Timeout 和 Resume | Approve/Reject/Timeout 恢复正确端口；重复动作不重复恢复；资格与租户校验生效 |
| INT-003 | M7-0 | done | EVA-005–010、RUN-014、AGT-012 | Evaluation Batch、Case Execution、评分和报告聚合 | 每个 Case Result 引用真实 Execution；耗时/成本/Trace 来自运行记录；取消可收敛 |
| INT-004 | M7-0 | done | APP-006–008、RUN-002/010–014、STU-014 | Webhook/Schedule/Poll、activate/deactivate 和 Deployment 生命周期接线 | 生产 Trigger 只运行已部署 Version；重复扫描/投递幂等；停用后不再创建新 Execution |
| INT-005 | M7-1 | done | RES-007/011、RUN-009、AGT-001–011、STU-003 | Execution 创建和 Node Attempt 的二次授权、撤权和资源状态策略 | 新执行在撤权后失败；有效运行按已固化 Handle 收敛；历史 Snapshot 可解释 |
| INT-006 | M7-1 | done | IAM-005、RUN-010–013、AGT-010、INT-001–005 | 租户限额、Reservation/Usage Ledger、限流和 Sandbox 资源强制 | 并发、Token、Cost、CPU、内存、PID、磁盘、TTL 无跨租户影响；崩溃后预留可回收 |
| INT-007 | M7-1 | done | OBS-003、INT-001–006 | 版本化 Runtime Event、幂等 Projector 和业务通知 | 重复/乱序事件不重复通知或回退终态；跳转定位正确业务对象 |
| INT-008 | M7-1 | done | FND-005、OBS-005–006、REC-001、INT-007 | Trace/Artifact/Message/Report 保留、Dry Run、引用检查和批次清理 | 被 Version/Checkpoint/Evaluation/Session/Comparison 引用的数据不删除；未引用 Message 和 Evaluation 可清理；失败批次、ObjectStore 和 ClickHouse 故障可重试 |
| INT-009 | M7-2 | in_progress | FND-003–010、INT-001–008 | RC Migration、Schema 兼容、Capability 门禁、滚动升级和应用回滚 | 新旧实例过渡期间状态无损；未知 IR 不被旧 Worker 领取；应用回滚可用 |
| INT-010 | M7-2 | done | RUN-010–013、AGT-008–012、INT-006 | Redis/Worker/Coordinator/ClickHouse/MinIO/OpenSandbox 故障矩阵和生产 RuntimeClass | 最终 MySQL 状态正确；实际 Pod 的 RuntimeClass、CPU/内存/PID/磁盘/TTL、IPv4/IPv6 Egress 和残留检查通过 |
| INT-011 | M7-2 | in_progress | IAM-007、RES-001、INT-001–010 | Vault/SecretProvider、API Key、Sandbox 凭证撤销、镜像/SBOM/签名和两租户攻击矩阵 | 无跨租户 ID 猜测、Grant 绕过、日志泄密或凭证重放；Sandbox 销毁后 Handle 失效；签名可验证 |
| INT-012 | M7-3 | in_progress | INT-001–011 | 容量基线、水平扩容、背压、长时间运行和大型 Workflow 测试 | Coordinator/Worker/Gateway/Sandbox/Trace Writer 可独立扩容；队列、SSE、Trace 和配额达到记录的首期基线 |
| INT-013 | M7-3 | done | STU-015–016、INT-001–012 | 全部正式页面的真实 API、错误/空状态、中文/英文、浅/深主题和桌面无障碍收口 | 无业务 Mock、未实现 Toast、硬编码文案和样式分叉；M6 Studio 交互不被改变 |
| INT-014 | M7-3 | in_progress | INT-001–013 | 全新集群安装、MVP 十二步、Release Manifest、镜像 Digest/签名、JUnit/HTML/Trace 证据和运维手册 | 追踪矩阵全部 done；`failures=0`、`skipped=0`；安装到业务闭环和回滚可重复通过 |

## 6. 依赖与关键路径

```text
INT-001 Application --+
INT-002 Approval -----+--> INT-007 Events --> INT-008 Retention --+
INT-003 Evaluation ---+                                          |
INT-004 Triggers -----+--> INT-005 Auth --> INT-006 Quota -------+--> INT-009 Upgrade
                                                                     +--> INT-010 Failure
                                                                     +--> INT-011 Security
INT-009/010/011 --> INT-012 Capacity --> INT-013 UI --> INT-014 Release
```

INT-001～004 可以并行，但必须复用同一 `ExecutionRuntime` 和 Event Envelope。INT-006 在 INT-005 授权边界稳定后实施；INT-014 只能使用 INT-001～013 的真实产物，不允许在验收脚本中临时绕过。

## 7. MVP 十二步发布场景

1. 空环境初始化企业和 Admin，并使用 JWT 登录。
2. 创建部门、用户、角色和数据范围。
3. 接入 Credential、Model、MCP Server/Tool、Skill Workspace、LightRAG 和 Mem0。
4. 创建 Workflow，并向 Workflow Service Identity 授权全部直接/递归依赖。
5. 只通过 Studio 拖拽和配置 Agent、MCP Tool、Code 与 Approval Workflow。
6. 以 Draft Revision 调试并查看节点 Input/Output、Attempt、Lineage 和 Trace。
7. 查看 Model、Tool、Agent Iteration、Token、Cost、Sandbox 和错误。
8. 从历史 Checkpoint 创建 Fork Execution，并处理副作用确认。
9. 发布 Version，用 Dataset Version 批量评测并查看真实 Case Report。
10. 将 Workflow Version 发布为 Application Deployment。
11. 通过 Playground 和 API 创建 Session/Message、真实 Execution 并恢复 SSE。
12. 在待办中心审批并确认原 Execution 从正确端口恢复。

步骤 1～8 验证 M1～M6 的成果没有回归；步骤 9～12 是 M7 的主要业务接线。整个场景必须由 UI 和公开 API 完成，数据库只用于最终断言。

## 8. Kubernetes E2E 与证据

- 每次完整验收创建唯一临时 Namespace；先确认目标路径后清理该 Namespace，不删除共享集群资源。
- 资源紧张时可按项目约束先缩容 `agentx` Namespace，但必须记录并在测试后恢复。
- 业务数据通过可见 UI/公开 API 创建；SQL 只允许环境准备、故障注入后的权威状态断言和清理。
- JUnit、HTML Report、Trace、Kubernetes 事件、Pod Spec、镜像 Digest/SBOM/签名和数据库断言按 Stage/Run ID 隔离保存；无环境运行不能覆盖完整证据。
- 故障测试至少覆盖 Worker 强退、Coordinator 重启、Redis 中断、ClickHouse 中断、MinIO 延迟、OpenSandbox 超时/残留、Vault 暂不可用和 SSE 断线。
- 安全矩阵使用两个租户/部门身份验证 Workflow、Execution、Artifact、Grant、Credential Handle、Approval、Application Key 和 Sandbox 网络边界。
- `scripts/m7-capacity.ps1` 生成版本化容量证据；`scripts/vault-integration.ps1` 生成真实 Vault KV v2 证据；`scripts/m7-release-gate.ps1` 只有在全部业务、容量、故障、安全、升级、隔离和供应链证据通过后才生成最终验收文件。

## 9. 发布门禁

- 全部 Cargo、前端 lint/test/build、OpenAPI、Node Schema、Migration、Kustomize 和部署组合检查通过。
- 所有正式入口都创建真实 Execution；未实现 Adapter 导致的 `RUNTIME_UNAVAILABLE` 数量为零。
- M6 Studio 冻结能力保持通过，M7 没有新增 Node Type 白名单或第二套执行路径。
- 配额、撤权、保留、事件幂等、故障恢复和两租户安全矩阵通过。
- 生产候选集群使用批准的 RuntimeClass、Vault/等价 SecretProvider、Digest 镜像、SBOM 和签名验证。
- MVP 十二步在全新集群可重复通过，阶段证据 `failures=0` 且 `skipped=0`。
- [功能追踪矩阵](99-feature-traceability.md) 无 `planned`、`in_progress` 或 `blocked`。

## 10. M7 之后的项目状态

M7 完成后，Agentx 已具备首期核心产品闭环：企业权限与资源管理、n8n 式但 Agentx 原生的画布配置体验、可靠 Workflow/Agent/Code 运行、调试与 Trace、评测、发布、Application/API、审批恢复以及生产安全和运维门禁。

后续里程碑属于产品扩展而不是首期补洞，例如更多 Agentx 原生节点、连接器模板、OIDC、商业计费、实时协作、跨区域容灾和更完整的运维观测。任何扩展不得反向把 n8n 协议兼容引入 M6/M7 的已冻结核心。
