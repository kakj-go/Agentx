# Mem0 Addon

`bundled` 使用 Mem0 `2.0.15` 本地构建镜像、PostgreSQL 和两个 PVC。Provider 地址/模型来自 Profile，Provider API Key、PostgreSQL 密码和 JWT Secret 位于 `agentx-mem0-secrets`。

registry 模式需要同时发布 `<registry>/mem0-server:v2.0.15`。`external` 不部署 Mem0/PostgreSQL，只检查 Endpoint；Bootstrap 后通过 UI/API 创建 Credential、Connection、测试连接和 Grant。Addon 卸载默认保留 `mem0-history-data` 与 `mem0-postgres-data`。

使用 `deploy.ps1 -Action Install|Upgrade|Uninstall -Target addons` 管理 Addon。外部实例不受脚本生命周期管理；删除本地 PVC 必须显式传入 `-DeleteData`。
