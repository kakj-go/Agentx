# MySQL

`bundled` 使用 MySQL 8.4 StatefulSet 和 `data-mysql-0` PVC。Secret 键为 `AGENTX_MYSQL_PASSWORD`、`AGENTX_MYSQL_ROOT_PASSWORD`。

外部模式在 Profile 中设置 host、port、database、user、TLS Mode 和 CA/mTLS 文件，并通过 managed 环境变量或 existing Secret 提供密码。Doctor 执行 `SELECT 1`，Migration 由 Platform API Job 执行。

```powershell
.\scripts\deploy.ps1 -Action Install -ConfigFile .\agentx.deploy.json -Target infrastructure
.\scripts\deploy.ps1 -Action Upgrade -ConfigFile .\agentx.deploy.json -Target infrastructure
.\scripts\deploy.ps1 -Action Uninstall -Namespace agentx -Target infrastructure
```

升级镜像或存储前先做逻辑备份。普通 Upgrade 不允许 bundled/external 切换；Uninstall 默认保留 PVC，`-DeleteData` 才会删除脚本拥有的 PVC。
