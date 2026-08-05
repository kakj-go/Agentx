# Agentx 服务清单

- `core/`：Web、Platform API、Trigger Gateway、Coordinator、Worker、Trace Writer。
- `migrations/`：MySQL 与 ClickHouse Migration Job。
- `sandbox-manager/`：仅 Sandbox remote 模式部署。

不要直接把这些目录当成完整安装包。它们依赖脚本生成的 `agentx-config`、`agentx-secrets` 和可选 `agentx-trust-bundle`。

```powershell
.\scripts\deploy.ps1 -Action Install -ConfigFile .\agentx.deploy.json -Target services
.\scripts\deploy.ps1 -Action Upgrade -ConfigFile .\agentx.deploy.json -Target services
.\scripts\deploy.ps1 -Action Uninstall -Namespace agentx -Target services
```

Sandbox Manager 使用 `-Target sandbox` 独立安装、升级或卸载。关闭 remote 模式和卸载前都会执行 `doctor-drain`。

卸载由统一脚本按所有权标签执行。Migration Job 可重复创建，升级前脚本会删除旧 Job 并等待新 Job 完成。
