# V2-06A Claim、Lease 与恢复审计目录

机器可读目录见 [claim-lease-audit.json](claim-lease-audit.json)。本目录覆盖所有紧凑 Profile 常驻后台循环，默认上限统一为 Lease 30 秒、Heartbeat 10 秒和批量 100；所有 MySQL 到期判断使用 `UTC_TIMESTAMP(6)`。

| Role | 权威队列/状态 | 协调方式 | 外部 I/O 边界 | 恢复来源 |
|---|---|---|---|---|
| Control Publisher / Admission | `publish_attempts` / `outbox` | `SKIP LOCKED` + Owner/Fencing | Claim 提交后调用 Runtime API/OSS | Attempt 下一动作、Outbox、Runtime Receipt |
| Control Projector / Retention | Cursor / `retention_runs` | 分区 Lease / `SKIP LOCKED` | Claim 提交后拉 Event 或发 Command | Cursor、Receipt、Generation、Retention Receipt |
| Runtime Command / Outbox / Sequencer | `runtime_commands` / `execution_outbox` / Cursor Row | `SKIP LOCKED` + 串行 Cursor | 提交后 Redis/SSE | MySQL Command、Outbox、唯一 `source_outbox_id` |
| Trigger / Recovery / Wait / Artifact | Binding、Attempt、Wait、Checkpoint | Lease、CAS、稳定对象键 | Provider/Redis/OSS 均在事务外 | Cursor、状态版本、Hash、幂等键 |
| Quota / GC / Retention / Trace Relay | Leader、GC Item、Retention Item、Trace Outbox | Leader Lease / `SKIP LOCKED` | Redis/OSS 在 Claim 提交后 | Ledger、审计清单、Trace Outbox |
| Worker / Sandbox | Attempt / Sandbox Lease | Redis Group + MySQL Lease / `SKIP LOCKED` | Provider 调用在 Claim 提交后 | Result Hash、Provider Label、稳定 Operation Key |
| Observability Consumer | Redis Pending | Consumer Group + Pending 接管 | ClickHouse 成功后 ACK | Pending Entry、`event_id + content_hash` |

静态门禁 `uv run --frozen pytest deploy/tests/test_contracts.py -k claim_lease` 校验目录完整性、源文件存在、批次上限、索引登记，并拒绝生产源码以 Pod 本地时间判断 `locked_until`；同文件的 Helm 测试校验 Pod UID 注入。动态的旧 Token 拒绝、强退接管和外部响应丢失必须由 `uv run --frozen pytest tests/e2e -m runtime --values <values.yaml>` 提供证据，静态目录不能替代 E2E。
