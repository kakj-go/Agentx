# OpenSandbox Docker Runtime

Docker Runtime 用于本地开发和功能 E2E，不由 `agentx-deploy` 安装。按 OpenSandbox 固定版本的官方 Docker 文档启动 Lifecycle Server，并配置 API Key、专用端口范围、默认拒绝网络、TTL、资源限制、`no_new_privileges` 和 Host Path 白名单。

Agentx Values使用 `global.components.sandbox.mode=external_opensandbox`，Endpoint通常为 `http://host.docker.internal:18080`，`secureAccess=false`。API Key通过 Runtime工作负载 Secret提供。

```bash
uv run --frozen agentx-deploy install --values deploy/values/local.yaml
```

升级前停止新任务并确认 Sandbox Lease已 Drain，再升级 OpenSandbox并执行 `agentx-deploy doctor`。卸载前列出并删除本次 Agentx标签创建的 Sandbox；主部署 CLI不会停止 Docker Runtime。
