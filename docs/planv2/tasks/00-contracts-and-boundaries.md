# V2-00 契约、所有权与代码边界任务清单

本阶段先冻结可执行边界，再开始拆库。来源任务为 [V2A-001～010](../03-refactor-phases.md#2-v2-00契约冻结和代码边界)。

## 1. 完成结果

- 当前每张表和每个后台循环都有唯一归属、替代方案和负责任务。
- Bundle、Work Package、Command、Event、Internal API 和版本窗口可以独立测试。
- Runtime 代码无法构造 Control Repository/Pool，反向越界也能被 CI 阻止。
- 所有后台任务复用统一 Claim/Lease/Fencing 语义。
- 原产品矩阵每一行都有明确的 V2 Owner 和回归入口。

## 2. 推荐批次

```text
A0 事实清点和逐表处置
 → A1 Spec/Admission、Bundle/Event/版本契约
 → A2 Repository/Settings/网络信任边界
 → A3 Claim/Lease 公共库
 → A4 静态扫描、产品矩阵和负向测试
 → A5 删除/替代清单冻结
```

## 3. 分步任务

| ID | 状态 | 依赖 | 实施步骤 | 交付物 | 可验证验收 |
|---|---|---|---|---|---|
| V2A-001 | done | 无 | 盘点构建产物、Role、端口、数据库、Redis、OSS、后台循环和调用链；给每项指定 Control/Runtime/Observability Owner | 架构 ADR、服务/端口/后台任务清单 | 无未归属常驻进程、端口或后台循环；与目标 7 类构建产物一致 |
| V2A-006 | done | V2A-001 | 从 Schema Catalog 和业务目录读取全部当前表；逐表填写 `control/runtime/split/delete`、Writer、Reader、事件、保留和任务；生成机器可读清单 | `table-disposition` 文档与 JSON/YAML 允许列表 | 当前表集合与处置目录集合完全相等，每张表恰好一次，无双权威 Writer |
| V2A-007 | done | V2A-001、006 | 冻结 Execution Spec、Admission State 和 Debug/Evaluation Work Package；定义 Bundle Reference、Epoch、TTL、对象闭包和 GC 关系 | Rust DTO、JSON Schema、状态图和测试 Fixture | Bundle 回滚不降低 API Key/Grant/Tenant/Quota Epoch；临时 Package 不回读 Control |
| V2A-002 | done | V2A-007 | 定义 Canonical Encoding、Hash、签名、Envelope、Inbox/Outbox、Cursor、Receipt、Prepare/Activate 和 Snapshot Export；默认传完整状态而非 Patch | `agentx-runtime-contracts`、Schema、Internal OpenAPI | 序列化、Hash、篡改、未知字段和旧版本拒绝通过 |
| V2A-010 | done | V2A-002 | 冻结 Bundle/IR/Event/Internal API/Worker Protocol 的当前与上一版本支持窗口及 Expand/Contract 顺序 | 版本兼容矩阵、升级/回滚决策表 | 初始只接受 1；后续当前与上一版本 |
| V2A-003 | done | V2A-001、006 | 拆出 Control/Runtime Settings、Repository、Migration 和 SQL Row 边界；公共 Domain/Application 不导出基础设施类型 | Control/Runtime Infrastructure Crate、依赖规则 | Runtime 构造路径没有 Control DSN/Pool 类型 |
| V2A-009 | done | V2A-001、002 | 冻结每个 Role 的数据/网络/API/幂等/依赖/终止/容量/恢复契约；定义 Service JWT Claim、Scope、双 `kid` 和 Delegation Token | 服务运行契约、调用矩阵、端口与连接预算 | JWT/Delegation 正负测试通过 |
| V2A-008 | done | V2A-001 | 实现公共 Claim/Lease：数据库时间、Owner、Deadline、Fencing Token、批次、Retry、`SKIP LOCKED` 和索引模板 | 公共库、双面 MySQL Fixture、使用指南 | 20 路 Claim、过期接管、旧 Owner/Token 和重复 Complete 通过 |
| V2A-004 | done | V2A-003、006、008～010 | 建立 Cargo/SQL/Env/Secret/NetworkPolicy/表名扫描；允许列表来自机器可读契约，不根据前缀猜测 | CI 边界脚本和负向 Fixture | 五类负向 Fixture 必须失败 |
| V2A-005 | done | V2A-002～004、006 | 列出旧 API、事件、Migration、Profile、Adapter、Projector 和共享表的唯一替代任务与最早删除点 | V1 删除台账 | 每个删除项有 Replacement、Owner、依赖和验证 |

## 4. 阶段内执行检查点

1. 先保存当前表、服务、端口、环境变量和依赖的机器快照，避免靠记忆建立边界。
2. 完成 V2A-001/006 后评审数据权威；出现双 Writer 时先决定业务所有权，不能交给同步机制掩盖。
3. V2A-007/002 先使用独立 Fixture 验证，不接入完整业务服务。
4. V2A-003 只拆 Infrastructure 和 Contracts；不要一次性复制全部 Domain/Application。
5. V2A-008 的公共库必须先被一个 Control Claim 和一个 Runtime Claim 的测试 Fixture 使用。
6. V2A-004 必须同时有正向允许和负向拒绝 Fixture，避免扫描器只能报误报。
7. 把 [原产品能力矩阵](../../plan/99-feature-traceability.md)逐行映射到 [V2 产品矩阵](../99-traceability.md)，不能使用一个笼统回归任务替代。

## 5. 阶段门禁

- V2A-001～010 全部 `done`，且上游计划和追踪矩阵状态同步。
- 表目录无遗漏、无重复、无未定 Writer，后台任务清单无未归属循环。
- Contracts 可以生成 Schema/OpenAPI，重新生成后仓库无漂移。
- CI 能证明 Runtime 不能依赖 Control Repository、DSN、表或 Secret。
- Claim/Lease 在并发、过期、时钟偏差和旧 Fencing Token 下通过。
- V2 容量阈值、最大控制面离线时间、事件保留和版本支持窗口已经冻结。

阶段证据摘要保存为 `docs/planv2/evidence/v2-00.md`，原始结果保存到 `artifacts/v2/<run-id>/v2-00/`。
