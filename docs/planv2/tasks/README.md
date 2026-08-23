# Agentx V2 分步实施任务索引

本目录把 [V2 破坏性重构阶段计划](../03-refactor-phases.md) 拆成可以领取、实现和验收的任务清单。上级文档继续定义架构事实和阶段边界；本目录只定义实施顺序、依赖、交付物、验证和证据。

## 1. 使用规则

1. 状态只使用 `planned`、`in_progress`、`blocked` 和 `done`，初始状态全部为 `planned`。
2. 领取任务前先检查 Git 工作树；已有修改默认属于用户，不能覆盖、回退或删除无关内容。
3. 任务依赖未完成时不得提前标记 `in_progress`。允许先做只读调查或契约 Spike，但不能提前合入依赖未闭合的生产路径。
4. 每个任务必须同时完成实现、自动化测试、文档和证据；只有代码或只有文档都不能标记 `done`。
5. 每个阶段合入后必须保持 Workspace 编译、空库 Migration、静态边界扫描及已迁移能力回归通过。
6. 替代链路通过后立即删除对应旧路径；不得长期保留 V1/V2 双写、双读或 Feature Flag。
7. 后端和前端生产源文件不得超过 2000 行。前端继续使用现有 `shared/ui`、`shared/components`、Tailwind Token、Radix 和 TanStack 体系。
8. 跨面任务必须回答：权威事实、Spec/Admission 归属、控制面离线行为、幂等/乱序、回滚、查询来源、版本窗口和 E2E。

## 2. 任务文件

| 顺序 | 文件 | 范围 | 进入条件 | 核心退出门禁 |
|---:|---|---|---|---|
| 1 | [V2-00 契约与边界](00-contracts-and-boundaries.md) | V2A-001～010 | 当前架构和 Schema Catalog 可读取 | 表归属、Contracts、Claim、信任和版本窗口全部冻结 |
| 2 | [V2-01 基础设施与 Schema](01-infrastructure-and-schema.md) | V2D-001～009 | V2-00 done | 两 MySQL、单 Runtime Redis、三 OSS 域和最小部署隔离成立 |
| 3 | [V2-02 Bundle 发布](02-bundle-publishing.md) | V2P-001～009 | V2-01 done | 基础 Workflow 在控制面离线时完整执行 |
| 4 | [V2-03 Runtime Gateway](03-runtime-gateway.md) | V2G-001～008 | V2-02 基础切片 done | 所有生产入口和生产业务状态不访问 Control |
| 5 | [V2-04 Runtime Engine](04-runtime-engine.md) | V2R-001～015 | V2-02 done；按切片依赖 V2-03 | 全部 Runtime 产品能力使用 Runtime 本地依赖闭环 |
| 6 | [V2-05 查询与可观测](05-query-and-observability.md) | V2Q-001～007 | 相应 Runtime 状态已迁移 | 列表/详情/Trace 单一来源和 Control Pull 稳定 |
| 7 | [V2-06 横向扩展](06-horizontal-scalability.md) | V2S-001～006 | V2-03～05 done | 所有常驻服务至少双副本通过竞争和强退测试 |
| 8 | [V2-07 Kubernetes 与安全](07-kubernetes-and-security.md) | V2K-001～006 | 数据和服务拓扑稳定 | Profile、NetworkPolicy、Secret、Migration、恢复和独立升级通过 |
| 9 | [V2-08A 本地收口 / 08B 生产认证](08-cutover-and-acceptance.md) | V2C-001～007 | 08A 允许延期 06B/07B 生产认证 | 08A 已删除 V1 并完成本地闭环；08B 关闭容量、安全、恢复和最终发布矩阵 |

V2-03 和 V2-04 可在 V2P-008 通过后按产品能力垂直并行，但 V2-05 只能为已经迁移到 Runtime 权威状态的能力建立查询或投影。

## 3. 关键路径

```text
逐表归属与服务契约
  → 独立空库和凭据/网络隔离
  → Bundle Builder + Prepare/Activate
  → Gateway→Execution→Worker→Query 基础切片
  → Gateway 与 Runtime 产品能力逐项迁移
  → Query/Event/Trace 边界
  → 多副本、容量和故障验证
  → Kubernetes 安全隔离
  → 删除 V1 并完成全量验收
```

禁止在基础垂直切片之前同时改写所有 Runtime 能力。先证明一个最小 Workflow 可以在控制面完全离线时执行，再沿用同一 Contracts、Inbox/Outbox、Claim/Lease 和 Query 骨架扩展其他能力。

## 4. 单任务完成定义

任务标记 `done` 前逐项确认：

- 依赖任务均为 `done`，实现没有新增未记录的架构边界。
- 正向、拒绝、重复、乱序、响应丢失和崩溃恢复场景按任务风险覆盖。
- 数据库变化有独立空库 Migration 测试；跨面变化有 Schema/OpenAPI 漂移检查。
- 多副本后台任务使用公共 Claim/Lease/Fencing 或 Consumer Group，没有私有无锁扫描。
- UI 变更使用统一组件和错误契约，具备 loading、empty、error、permission 状态测试。
- `cargo xtask check` 及阶段清单指定的验证通过；全局门禁以 [E2E 标准](../04-e2e-acceptance.md)为准。
- 证据包含命令、提交/镜像版本、环境规格、结果和敏感信息清理说明。
- 同步更新本任务状态、[阶段状态](../README.md#6-阶段状态)和[追踪矩阵](../99-traceability.md)；三处状态不得矛盾。

## 5. 证据约定

后续实现统一使用以下结构；V2-00 负责把它固化到脚本和 CI：

```text
docs/planv2/evidence/
  v2-00.md ... v2-08.md             # 可复现命令、环境和结论摘要

artifacts/v2/<run-id>/
  junit/ playwright/ api/ mysql/
  redis/ clickhouse/ kubernetes/ timeline/

apps/e2e/test-results/v2/<run-id>/   # Playwright 原始结果
```

证据不得保存 API Key、Vault Token、Credential 明文、Webhook Secret 或完整用户输入。临时 Kubernetes Namespace 必须带唯一 `run-id`；无论测试成功失败，都要恢复被缩容的开发 Deployment 并删除本次 Namespace。

## 6. 领取与交付模板

领取任务时在 PR/工作记录中填写：

```text
task_id:
depends_on:
authority_and_boundary:
changed_contracts:
failure_and_idempotency_cases:
tests:
evidence:
deletions_after_replacement:
```

若实施发现需要改变已冻结架构边界，先更新 [目标架构 ADR](../00-target-architecture.md)，说明影响，再调整阶段任务和追踪矩阵，不得仅在代码中形成新边界。
