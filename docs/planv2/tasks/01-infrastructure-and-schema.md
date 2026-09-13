# V2-01 分域基础设施与空库 Schema 任务清单

本阶段只建立可以独立验证的 Control、Runtime 和 Observability 数据/部署骨架。来源任务为 [V2D-001～009](../03-refactor-phases.md#3-v2-01分域基础设施和空库-schema)。

## 1. 进入条件

- V2-00 全部完成，逐表处置目录和服务运行契约已冻结。
- 当前开发数据明确不迁移；V2 使用空库初始化。
- Claim/Lease、表归属扫描、端口和跨面调用允许列表可复用。

## 2. 推荐批次

```text
D0 目录/配置和独立 Migration Runner
 → D1 Control/Runtime/ClickHouse 初始 Schema
 → D2 两 MySQL、单 Runtime Redis、OSS 三域
 → D3 ServiceAccount/Secret/NetworkPolicy
 → D4 Bootstrap、Doctor 和边界 CI
 → D5 删除 Control Redis
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2D-001 | done | V2A-006、008 | 根据 Control 处置清单建立 `deploy/migrations/control/0001_initial.sql`；加入租户、唯一键、FK、Outbox/Inbox/Claim 和保留索引 | Control 初始 Migration 与空库测试 | 空库可初始化；只有 Control 账号可访问；约束和高频查询索引测试通过 |
| V2D-002 | done | V2A-006～008 | 建立 `deploy/migrations/runtime/0001_initial.sql`；覆盖 Bundle/Admission、Gateway、Execution、Lease、Outbox、Query 和 Event Export | Runtime 初始 Migration 与状态约束测试 | 空库可初始化；状态转换、幂等键、Fencing、Head CAS 和 Outbox 原子性测试通过 |
| V2D-003 | done | V2A-001 | 重建独立 ClickHouse 初始 Migration；保留稳定 `event_id/execution_id` 和租户分区策略 | ClickHouse Migration 目录和 Runner | 新库创建、重复运行、去重和最小 Trace 查询通过 |
| V2D-004 | done | V2D-001～003 | 配置两个独立 MySQL Endpoint/Secret/Pool/Migration History；Runtime Redis 使用独立 Endpoint 和凭据；计算各 Role 连接预算 | Profile、Settings、Secret 模板和 Doctor | 同实例双 Schema 不被接受；交叉账号访问失败；Control 没有 Redis 配置 |
| V2D-005 | done | V2A-009 | 建立 Control/Runtime/Observability Bucket 或隔离 Prefix、账号和 IAM Policy；对象引用包含 Domain/Tenant/Hash/Size | OSS Policy、Fixture 和越域测试 | Control 不能覆盖 Runtime Object，Worker 不能读写 Control Workspace，Observability 权限最小化 |
| V2D-007 | done | V2D-004、005 | 建立三个服务域 Namespace、ServiceAccount、Secret、默认拒绝 NetworkPolicy 和独立 Migration Job 骨架 | 最小 Kustomize/Profile | Runtime Pod 无 Control DB Secret；Control Pod 无 Redis Secret；仅冻结 Internal API 正向可达 |
| V2D-008 | done | V2D-001～007 | 为三域建立独立 Bootstrap/Fixture，使用正式 API/Internal API 创建跨面业务对象，不做跨库 SQL Seed | Bootstrap 命令、测试身份和 Fixture | 三库可分别清空重建；Fixture 没有跨库连接或共享管理员账号 |
| V2D-006 | done | V2D-001～008 | 将机器可读表目录接入 SQL/Cargo/Env/Manifest CI；为每个服务使用最小权限数据库账号执行 Doctor | 边界 CI 和运行时 Doctor | 允许查询成功，禁止表/端口/Secret 访问失败；新增越界代码立即阻断 |
| V2D-009 | done | V2D-004、006～008 | 删除 Control Redis Settings、Secret、清单、Client 和 Key 协议；用 Control MySQL、Ingress、本地有界缓存替换对应语义 | Control Redis 删除清单及替代实现 | 仓库静态扫描无 Control Redis；Control Pod 无 Redis 环境变量且不能连接 Runtime Redis |

## 4. Schema 实施约束

- Control、Runtime 和 ClickHouse Migration 必须有独立 History/Lock，不能由一个管理员连接串行代跑三库。
- V2 初始 Schema 直接表达最终边界，不把旧 `migrations/mysql/0001～0025` 复制到两个目录。
- Runtime 当前状态由领域事务同步写入；不能建立依赖自身 Event Consumer 才可查询的状态表。
- 跨面 ID 只做逻辑关联，不建立外键；删除保护使用 Reference API、Tombstone、Hold 和最终复检。
- Migration Runner 同一目标只允许一个 Job；应用 Pod 不自动执行 Migration。

## 5. 阶段门禁

- 独立空库 Migration、重复执行检查和最小权限账号测试通过。
- Profile 实际部署两个 MySQL、一个 Runtime Redis、一个 OSS 服务和一个 ClickHouse；Control Redis 不存在。
- Runtime 环境中没有 Control MySQL 地址/密码，Control 环境中没有任何 Redis 地址/密码。
- OSS 三域越权、数据库越权、默认拒绝网络和允许 Internal API 均有正负测试。
- 临时 Namespace 清理和开发副本恢复在失败路径同样执行。

阶段证据摘要保存为 `docs/planv2/evidence/v2-01.md`，原始结果保存到 `artifacts/v2/<run-id>/v2-01/`。

## 6. 完成证据

V2D-001～009 已由 [V2-01 验收证据](../evidence/v2-01.md)关闭。最终 Kubernetes Run 为 `20260813t0021`，覆盖三域并发 Migration Replay、Bootstrap 幂等、实际 TLS/连接预算 Doctor、Control 租户外键、Runtime CAS/幂等/状态/Outbox 原子性、ClickHouse 去重、账号最小权限、三 Bucket 越域读写拒绝、Internal API/Provider/公开入口 NetworkPolicy 正负路径、Runtime Redis PVC 空库重建、清理和开发副本恢复。
