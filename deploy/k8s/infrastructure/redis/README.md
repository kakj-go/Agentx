# Redis

`bundled` 使用 Redis 7.4 StatefulSet、AOF 和密码认证，PVC 为 `data-redis-0`。应用通过独立 `AGENTX_REDIS_PASSWORD` Secret 认证，密码不写入 URL/Profile。

外部模式支持 `redis://`、`rediss://`、私有 CA 和 mTLS。Doctor 执行 `PING`。Redis 仅保存队列、租约和短期状态，不替代 MySQL 备份；故障恢复后由 Outbox 和权威状态补投。

基础设施的安装、升级和卸载统一使用 `deploy.ps1 -Target infrastructure`。外部 Redis 永不由脚本修改；bundled PVC 默认保留，只有显式 `-DeleteData` 才删除。
