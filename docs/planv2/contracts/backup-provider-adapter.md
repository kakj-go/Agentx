# V2 Backup Provider Adapter v1

V2-07A 的生产依赖均由外部平台托管。仓库内的 `v2-backup-restore.ps1` 负责安全前置、RPO/RTO 计时、Manifest 校验和证据落盘；实际快照、PITR、Bucket Version 恢复和 Redis 清空由平台 Adapter 完成。

Adapter 必须接受 `-Action`、`-Target`、`-BackupId`、`-ConfigFile` 和 `-RestoreTarget`，只向标准输出写一个 JSON Receipt：

```json
{
  "status": "passed",
  "recoveryPointUtc": "2026-08-16T00:00:00Z",
  "objectCount": 1,
  "contentSha256": "<64 lowercase hex>",
  "schemaVersionObserved": "control-0006"
}
```

Adapter 必须通过 Workload Identity 或各平面的 Backup Secret取权，不得要求部署器读取明文 Credential。相同 `BackupId + Target + Action` 必须幂等；Restore 默认使用新 Endpoint，原地恢复需要显式确认。Runtime Redis 不接收持久化备份，`Rebuild` 必须先清空目标 Redis，再等待 Runtime 从 MySQL 权威状态重建 Stream、Consumer Group、Pending 和 Quota Counter。

Receipt 只允许示例中的五个字段，不得附带 Provider 请求、Credential、Bucket Key、DSN 或任意原始响应。正式 E2E 通过独立 `RestoreTargetsFile` 为 Control/Runtime MySQL、三域对象和 ClickHouse指定新恢复目标，并对每个目标顺序执行 `Backup → Restore → Verify`；Runtime Redis只允许 `Rebuild`。
