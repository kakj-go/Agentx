# OpenSandbox Docker Runtime

Docker Runtime 用于本地开发和功能 E2E，不由 `scripts/deploy.ps1` 安装。按 OpenSandbox 固定版本的官方 Docker 文档启动 Lifecycle Server，并配置 API Key、专用端口范围、默认拒绝网络、TTL、资源限制、`no_new_privileges` 和 Host Path 白名单。

Agentx Profile 使用 `sandbox.mode=remote`，Endpoint 通常为 `http://host.docker.internal:18080`，`secureAccess=false`、`useServerProxy=true`，并将 Host 加入 `allowedHosts`。API Key 通过 `AGENTX_DEPLOY_OPENSANDBOX_API_KEY` 或 existing Secret 提供。

升级前停止新任务并等待 `doctor-drain` 通过，再升级 OpenSandbox。卸载前列出并删除本次 Agentx 标签创建的 Sandbox；主部署脚本不会停止 Docker Runtime。
