# ClickHouse

`bundled` 使用 ClickHouse StatefulSet 和 `data-clickhouse-0` PVC。外部模式配置 HTTP/HTTPS URL、database、user、密码 Secret 和可选私有 CA Bundle。

Doctor 执行 `SELECT 1`，Trace Writer Migration Job 管理表结构。ClickHouse 故障不能回滚 MySQL 中的 Execution 状态；恢复后 Trace Outbox 会继续投递。删除 PVC 前应先导出需要保留的 Trace。

基础设施的安装、升级和卸载统一使用 `deploy.ps1 -Target infrastructure`。外部 ClickHouse 永不由脚本修改；普通 Upgrade 不能切换 bundled/external。
