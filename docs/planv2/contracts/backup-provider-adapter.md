# V2 Backup Provider Adapter v1

生产依赖均由外部平台托管。`agentx-deploy backup|restore` 负责安全前置、RPO/RTO 计时、Manifest 校验和证据落盘；实际快照、PITR、Bucket Version 恢复由平台 Adapter 完成。

Adapter 必须接受 `--action`、`--target`、`--backup-id`、`--values` 和可选 `--restore-target`，只向标准输出写一个 JSON Receipt：

```json
{
  "status": "passed",
  "recoveryPointUtc": "2026-08-16T00:00:00Z",
  "objectCount": 1,
  "contentSha256": "<64 lowercase hex>",
  "schemaVersionObserved": "control-0006"
}
```

Adapter 必须通过 Workload Identity 或各平面的 Backup Secret 取权，不得要求部署器读取明文 Credential。相同 `backup-id + target + action` 必须幂等；Restore 默认使用新 Endpoint，原地恢复需要 CLI 显式传入 `--allow-in-place-restore`。Runtime Redis 不进入备份 Target；Redis 重建由领域 E2E 先清空临时目标，再等待 Runtime 从 MySQL 权威状态重建 Stream、Consumer Group、Pending 和 Quota Counter。

Receipt 只允许示例中的五个字段，不得附带 Provider 请求、Credential、Bucket Key、DSN 或任意原始响应。正式 E2E 为 Control/Runtime MySQL、三域对象和 ClickHouse 分别显式提供 `--restore-target`，并对每个目标顺序执行 `Backup → Restore → Verify`。
