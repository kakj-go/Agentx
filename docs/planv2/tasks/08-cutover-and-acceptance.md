# V2-08A 本地功能收口与 V2-08B 生产认证任务清单

当前状态：V2-08A `done`，V2-08 总阶段 `in_progress`。08A 已删除 V1 架构并在本地 Kubernetes 完成完整产品闭环、故障恢复和中等性能回归；生产 TLS/PITR、最终安全强化、正式容量与两小时稳定性延期到 08B。完成证据为 [Run `20260818-final4`](../evidence/v2-08.md)。

## 1. 进入条件

- V2-00～05 与 V2-06A 已 `done`；V2-06B 和 V2-07 生产认证允许延期，但不得借 08A 补写核心 Runtime 协议或 Claim/Lease。
- E2E-V2-001～012 已有可运行自动化，不依赖共享数据库 Fixture 创建被测业务事实。
- 08A 本地门禁固定为 50 并发 Execution、200 Attempt、100 SSE 和 30 分钟稳定运行；生产容量、RPO/RTO 和安全认证由 08B 冻结并执行。
- V1 删除台账中每个条目都有完成替代链路和回归证据。

## 2. 推荐批次

```text
C-1 公共 API 逐路径处置与替代链路补齐
 → C0 删除 V1 Schema/API/Profile/兼容层
 → C1 全新环境 Bootstrap 和 MVP 十二步
 → C2 Control 离线 + Runtime/依赖故障矩阵
 → C3 多副本、模糊提交、本地滚动兼容和中等性能回归
 → C4 产品能力等价回归
 → C5 文档重生成、矩阵关闭和发布审查
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2C-001 | done（[证据](../evidence/v2-08.md)） | V2A-005、V2-01～06A；生产认证延期 | Platform API 逐路径处置报告归零；共享 Migration、旧表/SQL、旧 Runtime Projector、旧 Web 代理、旧 Redis Key、兼容 Adapter/API/Event/gRPC/Profile 和双模 Flag 已删除 | 干净 V2 工作树、API 处置报告和删除报告 | 151/151 路径实现，`migrationRequiredPaths=0`、`deletionAllowed=true`；边界、Workspace 与空库门禁通过 |
| V2C-002 | done（[证据](../evidence/v2-08.md)） | V2C-001 | 从零安装三域；用 UI/公开 API/Internal Publish 完成企业初始化、资源、Workflow、发布、Application、Session、Evaluation、Approval 等闭环 | V2 Bootstrap、示例 Bundle 和 E2E Fixture | 不导入旧数据、不跨库 SQL Seed；Execution/Trace/治理投影可互相定位 |
| V2C-003 | done（[证据](../evidence/v2-08.md)） | V2C-002 | 停止 Web、Platform Control 并验证已发布 Runtime 入口；完整专项语义复验阶段 E2E | 控制面离线故障 Suite | 已发布调用继续完成；恢复后按 Cursor/Receipt 收敛 |
| V2C-004 | done（[证据](../evidence/v2-08.md)） | V2C-002 | 最终拓扑注入 Runtime MySQL、Redis、ClickHouse 故障，并复验 V2-04/06 的 OSS、Vault、OpenSandbox、Worker/Coordinator 专项矩阵 | Runtime 故障矩阵 | DB 故障无伪成功；Redis 可重建；CH 不影响终态；敏感依赖无明文回退 |
| V2C-005 | planned（08B） | V2S-001～006、V2K-006 | 08A 只执行本地混合代、模糊提交和中等性能门禁；正式容量、SSE 风暴和两小时稳定性留在最终拓扑执行 | 容量/稳定性/滚动兼容报告 | 08A 不产生生产容量结论；08B 达到冻结阈值并通过两小时残留门禁 |
| V2C-007 | done（08A 本地产品等价，[证据](../evidence/v2-08.md)） | V2C-002～004；容量部分延期 | 按 V2 产品矩阵执行 Workflow 5.0、资源、Gateway、Evaluation、Approval、Fork、Agent、Sandbox、Studio、国际化和安全删除回归 | 产品能力回归报告 | 本地功能行均有自动化路径和边界断言；生产安全/容量行明确指向 08B |
| V2C-006 | planned（延期到 08B） | V2C-001～005、007 | 完成生产容量、安全、恢复认证后生成最终发布架构、Runbook 和审查记录 | 最终文档和发布审查记录 | 生产矩阵无非 done 项；RPO/RTO、容量、供应链和强隔离证据可复现 |

## 4. E2E 执行清单

| 场景 | 对应任务 | 通过条件 |
|---|---|---|
| E2E-V2-001 发布与激活 | V2C-002（done） | Prepare/Activate/回滚/Admission 单调且无半激活 |
| E2E-V2-002 控制面完全离线 | V2C-003（done） | 持续已发布调用无非预期失败，恢复补投无重复 |
| E2E-V2-003 Runtime MySQL 故障 | V2C-004（done） | 无伪成功、孤立 Invocation 或不可解释状态 |
| E2E-V2-004 Redis 丢失重建 | V2C-004（done） | MySQL 权威不丢，Stream/Counter 重建并收敛 |
| E2E-V2-005 多副本竞争 | V2S-001～004（done，[06A 证据](../evidence/v2-06.md)） | 数据库断言业务事实唯一，传输重复幂等 |
| E2E-V2-006 滚动扩缩容 | V2S-005（done，[06A 证据](../evidence/v2-06.md)） | 无静默丢失，SSE/Lease/Quota/积压收敛 |
| E2E-V2-007 外部依赖故障 | V2C-004（done，本地语义） | OSS/CH/Vault/OpenSandbox 按冻结策略降级 |
| E2E-V2-008 授权传播 | V2C-007（done，本地语义） | Revoke、LKG、Staleness 和历史可解释性正确 |
| E2E-V2-009 查询边界 | V2C-007（done） | BFF/Runtime/Observability 单一来源和 Token 隔离 |
| E2E-V2-010 产品能力等价 | V2C-007（done，本地范围） | 本地产品矩阵逐行闭环且数据域隔离开启 |
| E2E-V2-011 模糊提交与乱序 | V2C-004/V2C-007（done，本地语义） | 同 Key 同结果，旧 Version/Epoch 不覆盖新值 |
| E2E-V2-012 滚动版本兼容 | V2C-005（planned，延期到 08B） | 当前/上一版本在生产等价拓扑混跑，超窗在执行前拒绝 |

所有场景必须 `failures=0`、`errors=0`、`skipped=0`。测试环境使用唯一的 Control/Runtime/Observability 临时 Namespace；失败路径也必须恢复开发副本并删除本次 Namespace。

08A 收口期间，Control 资源健康、MCP 发现和 MCP 调试不得为通过本地 NetworkPolicy 而放宽 Control→Provider Egress。浏览器仍调用既有 `/api/v1`；Platform Control 校验 IAM 后签发短期 Publisher JWT，Runtime Internal Resource API 使用版本化 Vault Reference 调用获准 Provider，Control 仅保存健康结果、工具目录和调试回执。

## 5. 发布前静态门禁

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `pnpm lint:web`
- `pnpm --filter @agentx/web test`
- `pnpm build:web`
- Control/Runtime OpenAPI、Bundle/Event Schema 和生成 TypeScript 无漂移。
- Control/Runtime/ClickHouse 空库 Migration 和 Profile/Kustomize 全组合通过。
- SQL、Env、Secret、数据库权限、NetworkPolicy、2000 行限制和 `git diff --check` 通过。
- 无 Kafka/Pulsar/NATS/Event Sourcing Framework、通用 Control Runtime Summary 或 V1 双路径残留。

当前仓库通用检查入口是 `uv run --frozen agentx-check`；V2 边界、Migration、Values、Helm 和 E2E 门禁必须由 Python/pytest 固化，最终证据不得只引用人工命令历史。

## 6. 08A 完成与 08B 最终门禁

- 08A 已关闭 `V2C-001～004` 和 `V2C-007` 的本地产品等价范围，并通过 E2E-V2-001～011 的本地功能/故障语义。
- 旧共享 Schema、旧 API/协议、兼容层、Profile、Runtime Projector 和 Control Redis 已删除。
- 当前架构文档已从实际 V2 代码、空库 Schema 和部署清单更新；证据完成清理与敏感信息检查。
- 08B 仍需关闭 `V2C-005`、`V2C-006`、`V2S-006` 和生产安全/恢复行，复验生产滚动兼容后才能把 V2-08 与 V2 总计划标记为 `done`。

08A 阶段证据摘要保存为 `docs/planv2/evidence/v2-08.md`，原始结果保存到 `artifacts/v2/<run-id>/v2-08/08a/`；08B 最终生产发布包再保存到 `artifacts/v2/<run-id>/release/`。
