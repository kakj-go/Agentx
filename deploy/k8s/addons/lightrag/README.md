# LightRAG Addon

`bundled` 使用固定 LightRAG 镜像和 `lightrag-data` PVC。Provider Base URL、模型和 Embedding 维度来自 Profile，API Key 来自 `agentx-lightrag-secrets`。本地 Full 使用 Echo MCP Fixture；test/production 禁止使用该 Fixture，必须提供真实 Provider。

`external` 只做集群内 Endpoint 连通性检查，不部署 Pod。Bootstrap 后在 Agentx UI/API 创建 Credential、LightRAG Connection、测试连接并创建 Grant。切换为 external/disabled 会删除 managed Workload/Config/Secret，但默认保留 PVC。

使用 `deploy.ps1 -Action Install|Upgrade|Uninstall -Target addons` 管理 Addon。外部实例的升级和卸载由其所有者负责，Agentx 脚本不会修改。
