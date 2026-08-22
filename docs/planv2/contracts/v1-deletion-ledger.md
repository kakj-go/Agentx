# V1 删除与替代台账

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；请使用 `agentx-deploy`、`agentx-check` 与 `pytest tests/e2e`。

V2-08A 已完成 V1 物理删除。当前工作树不保留冻结 Profile、双路由、兼容 Feature 或 Legacy Cargo 例外；历史 V1 名称只允许出现在本删除记录、历史证据和“禁止重新引入”的边界 Fixture 中。

| V1 资产 | 最终替代链路 | 状态 | 验证 |
|---|---|---|---|
| `agentx-infrastructure-legacy` / 原共享 Infrastructure | `agentx-control-infrastructure`、`agentx-runtime-infrastructure` 和类型化 Contracts | deleted | Workspace Cargo 图与 `agentx-boundary-check` 无 Legacy 依赖/例外 |
| `agentx-runtime-rpc`、旧 gRPC Command/Event、Credential Broker | `/internal/runtime/v1`、Runtime-local Worker Protocol、Bundle Binding/Handle | deleted | Contracts/OpenAPI 漂移与旧协议扫描通过 |
| V1 `platform-api` | `platform-control --roles=api,publisher,projector,retention` | deleted | `/api/v1` 151/151 Path 已实现，`migrationRequiredPaths=0`、`deletionAllowed=true` |
| Platform Runtime Projector、Runtime SQL/Redis/CH 查询和 Artifact Repository | Runtime Query BFF、Control Pull Projector、Observability Query | deleted | Control 无 Runtime SQL/DSN/Redis/CH；E2E-V2-009 通过 |
| V1 `trigger-gateway` | V2 `runtime-gateway` | deleted | `/gateway/v1` 与 `openapi/trigger-gateway.json` 仅由 V2 Runtime Gateway 承载/生成 |
| V1 `workflow-coordinator` | `workflow-runtime --roles=coordinator,trigger,command,outbox,recovery,artifact,quota,trace-relay` | deleted | Runtime Engine、Claim/Lease、故障恢复和控制面离线 E2E 通过 |
| V1 `workflow-worker` 服务包 | V2 `agentx-v2-runtime --bin workflow-worker` | deleted | Capability、Attempt Lease、Fencing、Provider 与结果重放测试通过 |
| V1 `sandbox-manager` 服务包 | V2 `agentx-v2-runtime --bin sandbox-manager` | deleted | OpenSandbox、Reaper、Lease、强退和对账测试通过 |
| V1 `trace-writer`、Trace Offset/Writer | Runtime Trace Relay + `observability` Trace Consumer/Query | deleted | Observability 无 MySQL Credential；CH 故障不影响终态且可补投 |
| 共享 `migrations/mysql` 和 `release_schema_contract` | `migrations/control`、`migrations/runtime`、`migrations/observability` 独立历史 | deleted | 最终空库历史 Control `1..7`、Runtime `1..7`、Observability `1..2` |
| 共享 `AGENTX_MYSQL_*`、`AGENTX_REDIS_*`、`AGENTX_S3_*` 和 `agentx-secrets` | 分面 typed Env、Secret、ServiceAccount 与 NetworkPolicy | deleted | Env/Secret/Kustomize/Profile 边界扫描通过 |
| V1 Kubernetes Stack、共享 Namespace、旧 Nginx 和部署脚本分支 | `deploy/k8s/v2` 三域 Base/Overlay 与 `scripts/deploy-v2.ps1` | deleted | Production/local Profile 渲染；最终 Namespace 无 V1 Deployment |
| 浏览器到 V1 Gateway/Coordinator 的代理与 Adapter | `/api/v1` Control BFF + 独立 `/gateway/v1` Runtime Host | deleted | Web/OpenAPI/生成类型无漂移；Control 离线 Runtime 入口继续工作 |
| V1 Echo MCP/Node Fixture Overlay | `deploy/k8s/v2/e2e/runtime-providers` | deleted/migrated | V2 Provider Overlay 可独立 Kustomize 渲染，不引用 V1 Stack |
| 仅服务 V1 的 Fixture/E2E/Release 脚本 | V2-08 API-first Bootstrap 与分阶段 V2 E2E | deleted | 被测业务事实由 UI、公共 API 或正式 Internal Command 创建 |

## 最终门禁

- [V2-08A 证据](../evidence/v2-08.md)：Run `20260818-final4`。
- API 处置报告：`artifacts/v2/20260818-final4/v2-08/08a/api-disposition.json`。
- `cargo check/clippy/test --workspace --all-targets`、`scripts/check.ps1`、V2 Profile/Kustomize 和 `git diff --check` 通过。
- `boundary-policy.json` 的 Legacy 例外为零；边界检查器继续保留旧名称的负向规则，防止 V1 被重新引入。

合法保留的名称不构成 V1 运行入口：`openapi/trigger-gateway.json` 是冻结的公共契约文件名，生成源为 V2 Runtime Gateway；`/internal/runtime/v1` 的 `v1` 是当前数字协议版本；`workflow-worker` 和 `sandbox-manager` 是 V2 二进制/Deployment 名称。
