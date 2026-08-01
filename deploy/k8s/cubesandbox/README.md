# CubeSandbox 本地接入

CubeSandbox 不是普通的无状态中间件。官方项目当前的标准 Kubernetes 部署仍处于预览阶段，计算节点需要满足虚拟化、内核和运行时要求，Docker Desktop Kubernetes 不能保证提供这些能力。

因此默认的本地 Kustomize 会启动 Agentx、MySQL、Redis、ClickHouse 和 MinIO，但不会部署一个不可工作的 CubeSandbox 占位 Pod。

本地开发时应在满足官方要求的 Linux 主机或 Kubernetes 集群上安装 CubeSandbox，然后把 AGENTX_CUBESANDBOX_ENDPOINT 配置为该环境的 Cube API 地址。正式接入时以官方部署包为准，不复制或维护一套非官方运行时清单。

参考项目：https://github.com/TencentCloud/CubeSandbox

