# OpenSandbox Kubernetes Runtime

在计算集群按 OpenSandbox 固定 Commit 的官方 Helm/Controller 文档安装 Lifecycle Server 与 Kubernetes Runtime。Agentx 只接入 Lifecycle Endpoint，不管理 OpenSandbox Namespace、Controller、RuntimeClass 或计算节点。

当前验收门禁覆盖 Kubernetes Pod 创建、资源限制、网络策略、TTL、取消、Manager 重启和清理。gVisor/Kata RuntimeClass 是后续生产隔离强化项；启用前应在目标集群验证镜像供应链、IPv4/IPv6 egress、Credential、跨租户隔离和节点容量。

升级顺序：先 drain Agentx Sandbox Lease，再升级 OpenSandbox 并运行 `doctor-opensandbox`，最后升级 Sandbox Manager。卸载前确认无活跃 Lease 和残留 Sandbox Pod。
