# LightRAG Addon

该目录是可选 Kustomize Addon，不属于四个 Agentx 核心 Helm Release。它使用固定 LightRAG 镜像和 `lightrag-data` PVC；Provider Base URL、模型和 Embedding 维度由 `agentx-lightrag-config` 提供，API Key 来自 `agentx-lightrag-secrets`。

E2E 组合清单会生成仅供测试使用的 ConfigMap 与 Secret，并通过 `kubectl apply -k deploy/kustomize/e2e-fixtures/runtime-providers` 显式安装。非测试环境必须在应用本目录前提供同名 ConfigMap 与 Secret；外部 LightRAG 由其所有者独立部署，Agentx 只通过 UI/API 创建 Credential、Connection 和 Grant。

升级使用同一条 `kubectl apply -k` 命令。卸载使用 `kubectl delete -k`，默认不会删除 `lightrag-data` PVC；数据删除必须由操作者单独确认并执行。核心 `agentxctl install|upgrade|uninstall` 不隐式管理本 Addon。
