# Platform API V2 切换契约

`platform-control` 是 V2 `/api/v1` 和 `openapi/platform-api.json` 的唯一实现与生成源。浏览器成功响应保持既有形状；Runtime 当前状态和 Trace 分别通过 Runtime/Observability BFF 获取，不允许 Control SQL 回退。

契约生成命令固定为 `cargo run -p platform-control -- openapi openapi/platform-api.json`；V1 `platform-api` 不再参与生成或漂移检查。

逐路径处置报告由以下命令生成：

```powershell
./scripts/v2-08-api-disposition.ps1 -OutputPath artifacts/v2/<run-id>/v2-08/08a/api-disposition.json
```

报告中的每个 OpenAPI Path 必须包含 Owner、数据平面、操作集合和删除门禁。只有 `migrationRequiredPaths=0` 且 `deletionAllowed=true` 时，才允许删除 `services/platform-api`。

实施期间允许报告包含 `migration_required`，但最终门禁必须使用：

```powershell
./scripts/v2-08-api-disposition.ps1 -FailOnMigrationRequired
```

不得通过从 OpenAPI 删除仍被 Web 使用的路径、增加 V1/V2 双路由或代理回 V1 服务来使报告归零。产品确认废弃的接口必须同时删除 Web 调用、生成类型和专项测试，并在 V2-08 删除报告中记录理由。
