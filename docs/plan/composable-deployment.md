# 可组合 Kubernetes 部署实施计划

状态：历史完成记录。实施日期：2026-08-04，最终验收日期：2026-08-05；其中 Profile、脚本和核心 Kustomize入口已在 Helm + Python切换中删除。

> 本文中的旧命令只记录当时验收事实，不能继续执行，也不代表当前部署接口。当前命令与资源所有权以 [Kubernetes 部署架构](../07-deployment.md) 和 [部署手册](../../deploy/README.md) 为准。

本文记录统一部署入口、外部分散依赖和独立 OpenSandbox 的交付边界。产品部署契约以 [Kubernetes 部署架构](../07-deployment.md) 为准，操作命令以 [部署手册](../../deploy/README.md) 为准。

## 1. 目标与边界

- 以 `scripts/deploy.ps1` 作为唯一编排入口，同时支持交互向导和 `agentx.io/deployment/v1alpha1` JSON Profile。
- MySQL、Redis、ClickHouse、Object Storage 必须选择 bundled 或 external，不能禁用。
- LightRAG、Mem0 可以 bundled、external 或 disabled；external 只检查 Endpoint，租户 Credential、Connection 和 Grant 仍由 Bootstrap 后的 UI/API 管理。
- OpenSandbox 始终独立安装。主脚本只在 `sandbox.mode=remote` 时部署 Sandbox Manager 并接入一个 Lifecycle Endpoint。
- 一个逻辑 Sandbox Manager 可以多副本共享 MySQL Lease；多 Provider、Docker Host 容量调度和 Sticky Routing不属于本阶段。
- gVisor/Kata、生产 Vault、镜像签名和生产级跨租户攻击隔离作为后续生产强化，不阻塞当前 Kubernetes 功能验收。

## 2. 交付批次

| 批次 | 状态 | 交付物 | 退出条件 |
|---|---|---|---|
| DEP-001 Profile 与目录 | done | Profile Schema、Full/Custom 示例；services、infrastructure、addons、fixtures、stacks、opensandbox 目录 | 576 种模式组合可校验；组件 Kustomization 不固定 Namespace |
| DEP-002 统一脚本 | done | Doctor、Install、Upgrade、Status、Uninstall；Target、DryRun、Secret 和所有权语义 | 安装顺序固定；状态 ConfigMap 不含 Secret；外部资源不被修改 |
| DEP-003 Ingress | done | 固定 ingress-nginx Chart/Controller/SHA、独立 IngressClass 和所有权标记 | 不接管已有 Release；仍有使用者时不卸载 Controller |
| DEP-004 外部配置 | done | 按服务 Settings、MySQL/Redis/ClickHouse/S3 TLS、CA、mTLS/Session Token | Sandbox Manager 不解析无关依赖；不支持跳过证书验证 |
| DEP-005 Doctor 与健康 | done | `doctor-infrastructure`、`doctor-opensandbox`、`doctor-drain`、周期 Readiness/Heartbeat | 安装在 Migration 前实际探测四项依赖；Manager 状态反映 OpenSandbox |
| DEP-006 文档与兼容 | done | 部署/组件/OpenSandbox 手册和旧脚本 Wrapper | Full、Custom、升级、卸载、备份、外部接入均有可复现命令 |
| DEP-007 自动化验收 | done | Profile 测试、Kustomize/Helm 渲染、分散依赖 E2E、完整 M2.1-M5 E2E | 两个临时 Namespace 清理；`check.ps1` 与 `git diff --check` 通过 |

## 3. 不变量

1. Profile 只保存模式、非敏感地址、CA 主机路径、镜像、Ingress 和 Kubernetes Secret 引用。
2. `agentx-deployment-state` 只保存规范化 Profile、SHA-256、最后 Target 和更新时间。
3. `secrets.mode=existing` 的 Secret 不由脚本轮换或删除；managed Secret 的 Credential Keyring 在轮换时保持不变。
4. bundled 持久化服务的数据库、存储和内部认证密码在通用轮换中保持不变，避免只改 Kubernetes Secret 却未改存量数据；外部凭据轮换必须先在 Provider 完成，再通过部署环境变量更新。
5. 普通 Upgrade 不改变四项状态型依赖的模式。模式迁移必须停写、备份、恢复并重新 Install。
6. Uninstall 的选择器必须同时匹配 `agentx.io/component` 与 `app.kubernetes.io/managed-by=agentx-deploy`；PVC 和 Namespace 还需要显式删除参数和所有权证据。
7. Sandbox disabled 时不部署 Manager、不注入 Manager URL/Token；Worker 继续领取 Sandbox capability 并返回 `RUNTIME_UNAVAILABLE`。

## 4. 验收矩阵

| 范围 | 自动化 |
|---|---|
| Profile/所有权 | `scripts/deploy-tests.ps1` |
| 静态渲染 | Full、E2E、组件 Kustomize；固定 ingress-nginx Helm Template |
| 分散依赖 | `scripts/deploy-distributed-e2e.ps1`，依赖与 Agentx 分属两个 Namespace |
| 功能回归 | `scripts/e2e.ps1`，覆盖 M2.1-M5、Addon、Agent、MCP、Code 和 Sandbox 故障 |
| 全量质量 | `scripts/check.ps1`、`git diff --check` |

最终验收结果：Profile 576 种模式组合通过；完整 M2.1-M5 Kubernetes E2E 返回 0；分散依赖 E2E 返回 `externalResourcesPreserved=true`、`upgradeDataPreserved=true`，并证明 Sandbox disabled 时 Code Execution 以 `RUNTIME_UNAVAILABLE` 快速失败、Sandbox Lease 为 0、Manager 未部署；TLS 常规测试 2/2、Docker 专项测试 1/1 通过；`scripts/check.ps1` 和 `git diff --check` 通过。

2026-08-05 从已删除的旧 Namespace 和 PVC 开始执行 `deploy.ps1 -Action Install -Profile Full -Namespace agentx -NonInteractive`，统一脚本成功创建部署状态、四项 bundled 基础设施、LightRAG、Mem0、核心服务和专用 Ingress；Full 默认 Sandbox disabled，因此未部署 Manager。分散验收结束后 `agentx-e2e` 与 `agentx-e2e-deps` 均已删除；`agentx-ingress` 因当前受管 `agentx` Ingress 仍在使用而按所有权规则保留。详细命令和证据见 [M5 验收证据](m5-acceptance-evidence.md)。
