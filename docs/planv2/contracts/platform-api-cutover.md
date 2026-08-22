# Platform API V2 切换契约

`platform-control` 是 V2 `/api/v1` 和 `openapi/platform-api.json` 的唯一实现与生成源。浏览器成功响应保持既有形状；Runtime 当前状态和 Trace 分别通过 Runtime/Observability BFF 获取，不允许 Control SQL 回退。

契约生成命令固定为 `cargo run -p platform-control -- openapi openapi/platform-api.json`；V1 `platform-api` 不再参与生成或漂移检查。

逐路径处置已经完成。当前回归门禁直接校验 OpenAPI 路径均有 `platform-control` Handler：

```bash
uv run --frozen pytest deploy/tests/test_contracts.py -k openapi_routes
```

历史处置报告中的每个 OpenAPI Path 必须包含 Owner、数据平面、操作集合和删除门禁；该报告已完成 `migrationRequiredPaths=0` 且 `deletionAllowed=true`，旧 `services/platform-api` 已删除。

不得通过从 OpenAPI 删除仍被 Web 使用的路径、增加双路由或代理回已删除服务来使门禁通过。产品确认废弃的接口必须同时删除 Web 调用、生成类型和专项测试，并在架构变更记录中说明理由。
