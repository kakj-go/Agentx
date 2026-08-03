# 阶段 12：全链路集成、加固和发布

## 1. 目标与用户价值

将前期外围控制面、运行引擎、Agent、Sandbox 和 Studio 接成完整产品闭环，并通过安全、故障、性能、升级和 Kubernetes 验收形成首期可发布版本。

## 2. 当前状态和进入条件

- 状态：`planned`。
- 进入条件：阶段 01 至 11 全部通过各自门禁。
- 本阶段不增加新的产品域，主要消除不可用 Adapter、Mock、孤立页面和未贯通事件。

## 3. 集成范围和不做内容

- Application Invocation 创建真实 Execution，SSE 和 Message 反映同一运行。
- Approval Node 创建真实 Task，审批动作恢复对应等待点。
- Evaluation Run 批量创建 Test Execution 并聚合真实指标。
- Checkpoint Fork、调试运行和 Studio 画布使用同一 Runtime。
- Notification 由发布、执行、审批、评测、成本和循环事件产生。
- 资源撤权、配额、保留和清理贯穿控制面与运行面。

不增加通用 OA、计费、插件市场、n8n JSON 导入、OIDC 登录或通用监控；这些能力不能成为首期发布阻塞项。

## 4. 领域对象、状态和不变量

- Release Candidate 固定应用镜像、Migration 版本、前端资源、API Schema 和节点能力集合。
- 一个业务请求只能产生一条权威 Invocation/Execution 链，Playground、API、Webhook、Schedule 和 Evaluation 不建立旁路。
- Feature 在真实存储、权限、API、页面和测试全部完成前不能标记 done。
- 所有终态业务对象保持可审计；重新运行、重新评测和重新发布创建新对象。
- 运行降级必须返回明确状态，不能以空数据、零成本或伪成功隐藏依赖故障。

## 5. 数据、Migration 和发布兼容

- 所有生产 Migration 必须向前兼容滚动部署，旧实例在过渡期间不能破坏新 Schema。
- Workflow Version、Dataset Version、Resource Version 和历史 Execution 不因升级改写。
- API 破坏性变化只能通过新版本或兼容字段演进。
- 服务横向扩展不依赖进程内 Session、Execution、Lease 或 SSE 权威状态。
- 保留清理先检查 Session、Checkpoint、Evaluation 和 Artifact 引用。

## 6. 配额和保留

实现租户级并发 Execution、Node Execution、Sandbox、单次时长、Agent Iteration、Token、成本、Artifact 大小和 Trace 保留。Coordinator 入队和 Worker 执行前均检查相应配额。

保留策略覆盖 Execution 摘要、Trace 内容、Prompt/Response、Artifact、Session Message 和 Evaluation Report，并提供 Dry Run、引用检查、批次删除和失败重试。

## 7. 公共 API、Port 和事件接线

- `ExecutionRuntime`、`EvaluationRuntime` 和 `ApprovalResumePort` 替换不可用实现，外围模块不改变调用方式。
- Application、Studio、Evaluation、Approval 和 Notification 统一使用 Runtime Event 与稳定业务 ID 关联。
- OpenAPI、内部命令、Redis Event 和 SSE Event 在 Release Candidate 中固化 Schema Version。
- 所有遗留 `RUNTIME_UNAVAILABLE` 只允许在依赖真实不可用时返回，不能来自未实现 Adapter。
- API 兼容测试验证当前前端和上一发布版本客户端的非破坏性调用。

## 8. 后端服务和前端收口

- Platform API 移除业务 Mock 和临时内存 Repository，保留测试专用 Fake。
- Gateway、Coordinator、Worker、Sandbox Manager 和 Trace Writer 接通完整事件链和 Readiness。
- 前端所有一级与详情页使用生成 Client，删除业务 Mock 数据和未实现 Toast。
- Dashboard、全局搜索、通知、租户切换和权限导航改用真实查询。
- Runtime Status 保持有限业务运行视角，不扩展基础设施监控产品。

## 9. Kubernetes 和运行加固

- Migration Job 在应用升级前执行。
- Readiness 阻止未就绪实例接收流量，Liveness 不依赖短暂外部故障。
- Worker 优雅停止时停止领取新任务并完成或释放已有 Lease。
- Coordinator、Worker、Gateway、Sandbox Manager 和 Trace Writer 可独立扩容。
- Redis、ClickHouse、MinIO 和 CubeSandbox 中断具有明确降级和恢复路径。
- 不要求 Prometheus/Grafana；产品运行状态页继续查询服务健康和业务队列状态。

## 10. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| INT-001 | planned | APP-005–011、RUN-014 | Application Invocation 到真实 Execution | Session Message、Invocation、Execution 和 Trace 可双向定位 |
| INT-002 | planned | OBS-001–003、REC-005–006 | Approval Node、Task、Notification 和 Resume 全链路 | Approve/Reject/Timeout 恢复正确端口且不重复 |
| INT-003 | planned | EVA-005–010、RUN-014、AGT-012 | Evaluation 批量 Execution、Profile 评分规则和报告聚合 | Case Result 引用真实 Execution，指标来自真实运行 |
| INT-004 | planned | APP-006–008、RUN-002、STU-009–013、REC-002–003 | Studio 调试、Checkpoint/Fork、版本发布和 Node Lifecycle 到 Trigger Gateway 接线 | 同一 Runtime 支持手动、API、activate/deactivate、poll、Webhook、Schedule 和 Evaluation |
| INT-005 | planned | RES-007、RUN-009、AGT-001–011 | 资源运行时二次授权和撤权策略 | 新执行在撤权后失败，历史记录保持可解释 |
| INT-006 | planned | IAM-005、RUN-010–013、AGT-010 | 租户运行配额和限流 | 并发、Token、成本和 Sandbox 配额无跨租户影响 |
| INT-007 | planned | OBS-003、INT-001–006 | 发布、执行、审批、评测、成本和循环通知 | 重复事件不重复通知，链接定位正确对象 |
| INT-008 | planned | FND-005、OBS-005–006、REC-001 | Trace/Artifact/Message/Report 保留与清理 | 引用中的 Artifact 不删除，批次失败可重试 |
| INT-009 | planned | FND-003–010、INT-001–008 | 滚动 Migration、兼容和回滚演练 | 新旧实例过渡期间读写兼容且无状态丢失 |
| INT-010 | planned | RUN-010–013、AGT-008–012 | Redis、Worker、Coordinator、ClickHouse、MinIO、Sandbox 故障测试 | 每类故障满足既定恢复时间和状态一致性 |
| INT-011 | planned | IAM-007、RES-001、INT-001–010 | 安全审查、租户隔离、Secret 和 API Key 测试 | 无跨租户 ID 猜测、日志泄密或越权资源调用 |
| INT-012 | planned | INT-001–011 | 性能、容量和水平扩展测试 | 队列增长、SSE、Trace 和大型 Workflow 达到首期容量基线 |
| INT-013 | planned | STU-014、INT-001–012 | 中英文、浅深主题、桌面端和无障碍验收 | 所有正式页面无 Mock、硬编码文案和样式分叉 |
| INT-014 | planned | INT-001–013 | MVP 十二步验收、发布清单和证据 | 追踪矩阵全部 done，安装到业务闭环可重复通过 |

## 11. MVP 十二步发布场景

1. 空环境初始化企业和 Admin，并登录。
2. 创建部门、用户、角色和数据范围。
3. 接入 Credential、Model、MCP Server/Tool、Skill Workspace、LightRAG 和 Mem0。
4. 创建 Workflow 并授权全部依赖资源。
5. 拖拽创建包含 Agent、MCP Tool、Code 和 Approval 的 Workflow。
6. 手动运行并查看节点输入输出、Attempt 和 Trace。
7. 查看模型、MCP Tool、Agent 循环、Token、成本和错误。
8. 从历史 Checkpoint 创建 Fork Execution。
9. 用 Dataset Version 批量评测 Workflow Version 并查看报告。
10. 发布 Workflow Version 为 Application Deployment。
11. 通过 Playground 和 API 创建 Session、Message，并接收 SSE。
12. 在待办中心审批并确认原 Execution 恢复。

## 12. 失败、安全和升级边界

- 故障测试必须验证最终 MySQL 状态，不能仅检查 HTTP 返回。
- 负载测试使用脱敏 Fixture，不使用生产 Secret 或客户数据。
- 数据清理任务以 Tenant 分片并设置删除上限，禁止广泛递归删除。
- 旧版本 Worker 不能领取需要新 Node Capability 的任务。
- CubeSandbox、ClickHouse 或 MinIO 降级时仅影响相应能力，不允许产生虚假成功结果。
- 发布回滚只回滚应用版本，已执行 Migration 必须保持向后兼容。

## 13. 测试和验收门禁

- 全部 Crate、服务、前端、OpenAPI、Migration 和 Kustomize 检查通过。
- MVP 十二步在全新本地 Kubernetes 环境可重复执行。
- 两租户安全矩阵、JWT/API Key、资源 Grant 和 Artifact 权限全部通过。
- Worker 强退、Coordinator 重启、Redis 中断、ClickHouse 中断和 Sandbox 超时均有证据。
- 前端所有正式页面使用真实 API，支持中文/英文、浅色/深色和桌面分辨率。
- [功能追踪矩阵](99-feature-traceability.md) 无 planned、in_progress 或 blocked 项。

## 14. 对发布的稳定输出

- 可安装和滚动升级的 Kubernetes 应用。
- 完整的 Workflow 开发、运行、评测、发布、审批和恢复闭环。
- 安全、故障、性能和容量验收证据。
- 版本兼容、数据保留和发布回滚操作说明。
