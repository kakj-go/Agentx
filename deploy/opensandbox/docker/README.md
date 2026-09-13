# OpenSandbox Docker Runtime

Docker Runtime 用于本地开发和功能 E2E，不由 `agentxctl` 安装。按 OpenSandbox 固定版本的官方 Docker 文档启动 Lifecycle Server，并配置 API Key、专用端口范围、默认拒绝网络、TTL、资源限制、`no_new_privileges` 和 Host Path 白名单。

Lifecycle Server 自身运行在 Docker 容器内时，`[docker]` 必须配置 `host_ip = "host.docker.internal"`。Server 使用该地址检查映射到宿主机随机端口的 egress sidecar；缺少该项会错误访问 Server 容器自身的 `127.0.0.1`，导致所有带 `networkPolicy` 的 Sandbox 在 30 秒后创建失败。

Docker Desktop 本地端口池固定为 `20000-29999`，避开 Windows 动态端口范围。不要改回 OpenSandbox 默认的 `40000-60000`，否则并发 E2E 中可能把 execd 或 egress 映射到已占用端口，表现为随机 TLS/PTY 超时。

Agentx Values使用 `global.components.sandbox.mode=external_opensandbox`，Endpoint通常为 `http://host.docker.internal:18080`，`secureAccess=false`。API Key通过 Runtime工作负载 Secret提供。

```bash
agentxctl install --values deploy/values/local.yaml
```

升级前停止新任务并确认 Sandbox Lease已 Drain，再升级 OpenSandbox并执行 `agentxctl doctor`。卸载前列出并删除本次 Agentx标签创建的 Sandbox；主部署 CLI不会停止 Docker Runtime。
