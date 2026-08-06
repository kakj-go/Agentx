# M7 全链路闭环与首期发布验收证据

状态：`in_progress`。当前复核日期：2026-08-06。最新业务闭环 Kubernetes Run ID：`20260806T075729954Z`。

M7 的 Application、Session、Message、Webhook、Approval Resume 和 Evaluation 已接入同一个 Version Execution Runtime，配额、保留、Worker Capability、Vault Broker、Runtime Event Projector 和发布供应链代码也已落地。完整业务闭环、撤权、故障、双租户安全、Retention 全引用矩阵和页面矩阵已经通过；M7 尚未标记为 `done`，因为容量、升级/回滚和签名镜像证据还未全部生成。

## 1. 当前任务状态

| 任务 | 状态 | 当前证据与剩余边界 |
|---|---|---|
| INT-001 | done | Playground/API 返回 `202 queued`，Command 异步创建真实 Execution；Session、Assistant Message、SSE Cursor 和 Trace 双向定位通过 |
| INT-002 | done | Approval 决策与 Resume Command 同事务；领取、通过、恢复、重复命令和不可恢复 Wait 校验通过 |
| INT-003 | done | Case 创建真实 Version Execution，JSON Schema 规则评分和报告聚合通过 |
| INT-004 | done | 最新 Run 通过 Webhook、Schedule、Poll、Lifecycle activate/deactivate 和停用校验；Poll/Schedule 各 1 次，停用 Application 后无活跃 Binding，重复扫描和 `fire_once`/`skip` misfire 证据已归档 |
| INT-005 | done | Kubernetes 公开 API 验证撤销 Model Grant 后新 Invocation 立即失败，恢复固定版本 Grant 后新执行完成；Testcontainers 验证 Retry 重新授权、原 Attempt Handle 在有效 Lease 内收敛、跨 Attempt/租户和重放被拒绝、Lease 释放后不能续签 |
| INT-006 | done | 12 个 Quota 维度共用单一清单；MySQL+Redis 双租户测试覆盖并发、Token、Cost、Artifact、CPU、内存、PID、磁盘和 TTL 的 Reservation/Ledger、周期用量、释放、过期回收与校准，Sandbox 资源强制和双租户攻击矩阵已通过；容量后零漂移继续由 INT-012 验收 |
| INT-007 | done | Runtime Event v1、Outbox、Receipt 和业务 Projector 已通过完整 Run；重复/乱序 Projector 单测和故障矩阵通过，未投影事件、终态缺失事件和未发布 Outbox 均为 0 |
| INT-008 | done | Retention Reference、Dry Run 和批次清理已实现；MySQL + InMemory ObjectStore 覆盖引用阻止、对象存储中断后恢复、501 条跨 6 批清理和幂等删除；ClickHouse Trace 分段失败恢复、Checkpoint 保护、Application Message 清理、Evaluation Case/Metric/Report 清理和 Comparison 引用保护均通过 |
| INT-009 | in_progress | 0016 expand、0017 contract、Worker Capability 分流、Digest Profile 和 Docker Desktop 双阶段验收器已实现；尚未生成 M6→M7、M7 Previous→Candidate 滚动及回滚的真实本地证据 |
| INT-010 | done | 故障矩阵覆盖 Coordinator、Worker、Redis、ClickHouse、MinIO、OpenSandbox、Vault、SSE 和 Projector；Docker Desktop runc 基线隔离检查通过并记录 `isolationLevel=standard` |
| INT-011 | in_progress | Vault KV v2、内部一次性 Broker、Handle 撤销、本地 TLS Registry、7 镜像 SBOM/签名/Attestation 和 8 项负向验收器已实现；尚未生成真实本地产物 |
| INT-012 | in_progress | 容量验收器已实现；100 Execution、500 Node、200 SSE、1000 Case、200 节点和 2 小时结果尚未执行 |
| INT-013 | done | Playground、Application Trigger、Evaluation、Approval 和 Runtime 页面均接真实 API；中英文、浅深主题、3 种桌面尺寸共 60 张截图通过横向溢出和可访问名称检查 |
| INT-014 | in_progress | 临时 Namespace 业务闭环、故障、安全、标准隔离和页面证据通过；升级、回滚、容量和签名证据尚未满足最终发布汇总器 |

## 2. 已通过的业务闭环

执行：

```powershell
.\scripts\e2e.ps1
```

Run `20260806T075729954Z` 保存 7 个阶段 JUnit 文件，共 14 条测试，`failures=0`、`errors=0`、`skipped=0`。M7 用例只依赖 M6 通过 UI 发布的 Workflow，通过公开 API/UI 完成：

- 创建 Application 和 Version Deployment，在 Playground 创建 Session 并发送 Message。
- 断开第一次 SSE 后使用 Cursor 恢复，最终定位真实 Execution、Trace 和 Assistant Message。
- 同一 Idempotency Key 重放返回同一 Invocation，不重复创建 Execution。
- 创建并验证 HMAC Webhook，返回 `202 queued` 并运行同一 Version。
- 创建 Dataset、Dataset Version、Evaluation Profile 和 Evaluation；Case 绑定真实 target Execution 并产生规则结果。
- 通过 Approval 页面领取和批准任务，Coordinator 通过 Runtime Command 幂等恢复执行。
- 查询 Worker Capability，更新 Quota Policy，运行 Retention Dry Run，并在 Runtime 页面显示真实状态。
- 最终 `runtime_commands` 和未发布 Outbox 清零，Sandbox Lease 终止，Credential Handle 撤销。

证据目录：`apps/e2e/test-results/kubernetes/20260806T075729954Z/`。该 Run 保留 `agentx-e2e` Namespace 供后续 RC 检查，开发 `agentx` Namespace 已恢复并保持 10 个 Deployment、4 个 StatefulSet 全部 Ready。该 Run 同时覆盖 Coordinator/Worker 强退、Redis/ClickHouse 故障、SSE 重连和 M5 Sandbox 清理。

专项运行证据：

- 故障矩阵：`artifacts/m7/20260806T081803101Z/failure/failure-evidence.json`，全部 10 项通过。
- 双租户安全矩阵：`artifacts/m7/20260806T081953676Z/security/security-evidence.json`，全部 7 项通过。
- runc 隔离基线：`artifacts/m7/20260806T075729954Z/isolation/isolation-evidence.json`，`isolationLevel=standard`。
- 页面矩阵：`apps/e2e/test-results/kubernetes/20260806T075729954Z-m7-visual/m7-business-closure-visual/`，1 条业务测试和 60 张截图通过。
- 运行时撤权矩阵：`apps/e2e/test-results/kubernetes/20260806T075729954Z-m7-revoke3/m7-business-closure-revoke3/`，1 条测试通过，`failures=0`、`errors=0`、`skipped=0`；失败路径不会遗留可重试 Runtime Command，测试退出前总会恢复 Grant。

## 3. Vault 和发布自动化

执行：

```powershell
.\scripts\vault-integration.ps1
```

真实 Vault KV v2 Dev 容器固定为 `hashicorp/vault@sha256:1262354cd28697b7982ea3b9b6f159a996bdaad0b5270765e31b67797ce15bea`。Run `20260806T030509886Z` 已验证写入 v1、轮换 v2、按版本读取、销毁 v1 后不可读取且 v2 保持可用；另有单元测试证明 Vault 不可用时不会回退本地密文。脚本只持久化结构化 Evidence，不保存可能包含 Dev Root Token 的容器日志。

新增发布自动化：

- `scripts/m7-capacity.ps1`：真实执行 100/500/200/1000/200 和默认 2 小时稳定性门禁，统计 Projector p95 与活跃 Quota Reservation。
- `scripts/verify-runtime-isolation.ps1`：检查 Kubernetes Sandbox 工作负载 Pod 的 RuntimeClass、non-root、只读根文件系统、ServiceAccount、seccomp、Capability、资源限制和 NetworkPolicy。当前本地证据选择 `sandbox-manager` Pod；宿主侧 OpenSandbox 执行实例由 M5 Oracle 和残留检查覆盖。
- `scripts/release-images.ps1`：解析镜像 Digest，生成 CycloneDX SBOM，Cosign 签名/证明并生成 Release Manifest。
- `scripts/m7-local-prereqs.ps1`：固定并校验 Docker Desktop Kubernetes、kubectl、Cosign 和 Syft 工具链及 SHA-256。
- `scripts/m7-local-release-tests.ps1`：在独立 TLS Registry/Test Namespace 中编排 INT-011、M6→M7 Schema 迁移、M7 滚动/回滚、持续 Invocation 探针、秘密扫描、JUnit 和证据清单。
- `scripts/m7-release-gate.ps1`：强制汇总 7 个 JUnit/HTML/Trace、容量、Vault、故障、安全、升级、隔离和供应链证据；任何阈值不足都不会生成 `passed` 文件。

`scripts/release-tests.ps1` 已验证上述脚本语法、证据 Schema、Digest 约束和 runc 不得声明 `strong`。

本轮静态门禁：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（全部可执行测试通过，3 个明确标记为 Docker/Vault/mTLS 环境依赖的测试 ignored）、Web lint 通过、Web Tests 19 个文件/42 个测试通过、Web production build 通过、Deployment Profile 576 组合测试通过、Full/E2E/Add-on Kustomize 渲染通过，Platform/Gateway/Node OpenAPI、Node Schema 和前端 TypeScript 生成物均无漂移。

代码级治理测试：

- QuotaAdmission Redis Lua 并发/幂等/释放：`cargo test -p agentx-infrastructure quota::tests::redis_admission_is_atomic_idempotent_and_releasable -- --ignored --exact`。
- Quota 12 维双租户、Ledger、过期回收和 Redis/MySQL 校准：`cargo test -p agentx-infrastructure --test quota_storage`。
- Retention 引用阻止与对象存储故障恢复：`cargo test -p agentx-infrastructure --test retention_storage`。
- Schedule IANA/DST 与重复时刻策略：`cargo test -p trigger-gateway schedule_loop::tests`。

本轮补充的可靠性证据：Redis Quota admission 使用 Lua 原子计数，32 并发、10 个限额、幂等重放和释放校准通过；Redis 客户端连接超时 2 秒、响应超时 5 秒并采用有界重试，故障时不会把 Gateway 请求拖至 Nginx 504。Poll 成功确认要求持有扫描锁，并按 Provider Event ID/Cursor 或响应 Hash 生成稳定幂等键。Retention 删除在 Version、Checkpoint、Evaluation Comparison、Session 和子 Execution 引用存在时拒绝；未引用的 Application Message、Evaluation Case/Metric/Report 可删除；对象存储中断、501 条跨批处理和 ClickHouse 分段失败均可恢复。

生产启动安全收口：`AGENTX_ENV` 或兼容的 `AGENTX_ENVIRONMENT` 为 `production` 时，Platform API、Trigger Gateway、Workflow Worker 和 Sandbox Manager 必须显式配置 `AGENTX_SECRET_PROVIDER=vault_kv_v2`；缺失、`local_encrypted` 或未知值会在启动阶段失败。配额 fail-closed 和开发默认 Quota 使用同一环境判定。定向配置测试 4/4 通过，工作区 `cargo check` 通过。

## 4. 尚未关闭的发布门禁

INT-009/011 采用已锁定的本地验收边界：在 Docker Desktop 单节点 Kubernetes、TLS 本地 Registry 和 runc 上执行以下命令，生成 `m7-local-evidence.json` 且 JUnit `failures=0`、`errors=0`、`skipped=0` 后，可将两项标为 `done`：

执行前还必须启动本机 OpenSandbox，并确保 `http://127.0.0.1:18080/health` 和带 API Key 的 `/v1/sandboxes` Lifecycle API 可访问；默认 Key 为本地 E2E 使用的 `agentx-local-opensandbox-key`。也可以通过验收脚本的 `-OpenSandboxEndpoint` 和 `-OpenSandboxApiKey` 参数覆盖，前置检查失败时不会进入镜像构建或升级阶段。

```powershell
.\scripts\m7-local-release-tests.ps1 `
  -RegistryHost <docker-desktop-node-internal-ip> `
  -RegistryPort 30500 `
  -CosignPrivateKey C:\secure\agentx-m7-local-cosign.key `
  -CosignPublicKey C:\secure\agentx-m7-local-cosign.pub
```

该命令只接受不同于 `f343333` 的已提交 Candidate；默认通过公开 API 创建阶段 A/B 的独立 Workflow、Version、Application 和 Execution，Token 不落盘。成功证据只代表 `local-docker-desktop`、`local-tls`、`trustScope=local-only` 和 `isolationLevel=standard`，不代表生产长期信任根或强隔离。

当前尚未执行上述完整 Run，因此 INT-009/011 仍为 `in_progress`。之后仍需完成：

1. 100 并发 Execution、500 Node Execution、200 SSE、1000 Evaluation Case、200 节点 Workflow 和 2 小时稳定性运行，Projector p95 `<5s` 且 Quota Reservation 最终为零。
2. INT-014 最终 Release Gate 汇总；它继续依赖既有业务、Vault、故障、安全、隔离、本地升级/供应链证据和 INT-012 容量证据。

本地 Kubernetes 使用 Docker+runc，只能记录 `isolationLevel=standard`。在 gVisor、Kata 或经评审的 custom RuntimeClass 上通过真实 Pod 验证后，才允许记录 `strong`。

## 5. 最终完成动作

本地 INT-009/011 证据通过后先只更新这两项；INT-012 仍保持 `in_progress`。全部证据齐备后运行 `scripts/m7-release-gate.ps1`，只有该脚本生成符合 `agentx.io/m7-acceptance-evidence/v1` 的 `passed` 文件，才同步执行以下最终状态变更：

- INT-012、INT-014 改为 `done`；若 INT-009/011 尚未有有效本地证据，Release Gate 必须拒绝。
- 阶段 12 和里程碑 M7 改为 `done`。
- `99-feature-traceability.md` 不再存在 `planned`、`in_progress` 或 `blocked`。
- 本文状态改为 `done`，记录最终 RC Run ID、Release Manifest 和回滚证据路径。
