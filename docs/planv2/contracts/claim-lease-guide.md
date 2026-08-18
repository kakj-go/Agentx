# MySQL Claim / Lease / Fencing 使用规则

公共库 `agentx-mysql-lease` 只定义语义、类型、常量和测试 Harness；各领域 Repository 必须保留静态 SQL 和自身状态约束。

1. 使用 `UTC_TIMESTAMP(6)` 计算 Claim、Heartbeat 与过期，不使用 Pod 时钟。
2. 在短事务中 `SELECT ... FOR UPDATE SKIP LOCKED`，生成不可复用 Owner UUID，将 Fencing Token 单调加一，并设置 `locked_until`。
3. 提交 Claim 事务后才能执行 Redis、OSS、Provider、HTTP、gRPC 或其他外部 I/O。
4. Heartbeat/Complete/Fail 的 `WHERE` 必须同时匹配 ID、运行状态、Owner、Fencing Token 和 `locked_until > UTC_TIMESTAMP(6)`；影响行数不是 1 时返回 `LeaseLost`。
5. Complete/Fail 必须清空 Lease；重复 Complete、旧 Owner、过期 Lease 和旧 Token 均返回 `LeaseLost`。
6. 默认 Lease 30 秒、Heartbeat 10 秒、批次最大 100。Role 可以收紧，不得放宽超过任务超时。
7. 推荐索引以领域状态开始并覆盖 `available_at,locked_until`；正式字段/索引在 V2-01 初始 Schema 中落地。

`tests/mysql_protocol.rs` 使用两个隔离 MySQL 临时表模拟 Control 与 Runtime Repository，验证 20 路并发 Claim、Pod 快/慢本地时钟不参与决策、Claimant 强退、数据库时间过期接管、旧 Owner/Token、Fail 和重复 Complete。测试表不属于正式 Migration。
