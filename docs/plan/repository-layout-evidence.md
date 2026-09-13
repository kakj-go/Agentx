# 仓库目录归并验收记录

状态：已完成（2026-09-13）。目录与工程改动、完整仓库门禁、26 项 Kubernetes E2E、Argus/历史 E2E 清理及正式 Agentx 环境恢复均已闭合。

实施计划见 [目录归并计划](repository-layout.md)。首次实施证据位于 `.local/artifacts/repository-layout/20260912-layout/`；2026-09-13 的清理、恢复及最终验收证据位于 `.local/artifacts/repository-layout/20260913-closure/`。

## 文件与数据保护

- 迁移前保存 1,251 个已有源码文件的完整 ZIP、Git 状态、暂存/未暂存补丁和 SHA-256。
- 已按目标结构归并源码、测试、工具、协议和现行 SQL，删除 `skills-lock.json`、三份旧 ClickHouse SQL 及空旧目录。
- 94,035 个历史证据文件已迁移并逐个校验内容摘要，保留日志、图片、Trace、视频与报告内部相对关系。
- 现行 SQL、OpenAPI、JSON Schema、OpenSandbox Spec 和图片基准的内容摘要未变化。
- 历史源码快照与删除记录保留原内容；机器消费的有效源码路径已更新。旧路径定位见计划中的映射表。
- 未自动暂存、提交、推送或发布，正式 Agentx PVC 未删除。Argus 专属 PVC 按 2026-09-13 的追加清理授权删除，见下节。
- 最终核对：885 个源码文件搬迁，主要工程目录为 6 个、根文件为 16 个；原有源码无遗漏，暂存区仍为空。

## 当前验证结果

| 检查 | 当前结果 | 证据 |
|---|---|---|
| Cargo 工作区识别 | 24 个成员，包名和二进制名保留 | `cargo-metadata.json` |
| npm 冻结安装 | 8 个子包；496 个包从本地缓存安装，第三方 packages/snapshots 解析内容不变 | pnpm 冻结安装输出 |
| 全成员编译 | `cargo check --workspace --all-targets --locked` 通过 | `cargo-check.log` |
| Clippy | workspace/all-targets、`-D warnings` 通过 | `clippy-final.log` |
| 快速统一门禁 | `cargo xtask check --fast` 通过 | `xtask-fast-final.log` |
| 边界检查器测试 | 3 个单元测试、5 个集成测试通过；覆盖新扫描根、缺失/空目录与违规反例 | `boundary-tests.log` |
| 仓库生产边界 | 实际新目录检查通过 | `boundary-repository.log` |
| Runtime 与契约 | 118 tests passed，包含 Schema 与 Studio Catalog 漂移检查 | `contract-runtime-tests.log` |
| Python acceptance | 25 passed | `acceptance.log` |
| 系统 E2E 收集 | 26 tests collected | `e2e-collection.log` |
| Windows 发布包 | release 构建、五文件归档、SHA-256、仓库外 validate/render 通过 | `windows-release-build.log`、`windows-release-smoke.log` |
| Web lint/build | 通过；保留既有大 bundle 提示 | `web-lint.log`、`web-build.log` |
| Web 测试 | 限制为 2 个 worker 后全部通过：88 files / 397 tests；统一门禁使用相同并发上限 | `web-tests.log`、`web-tests-serial.log` |
| 插件与浏览器类型 | 浏览器 TS、SDK/UI/Runner/内置包/模板 check/build 通过；SDK 3、Runner 10、Data 3、HTTP 1、模板 1 项测试通过，模板打包成功 | `plugin-checks.log` 及各包独立 `*-tests.log`、`template-pack.log` |
| 完整仓库门禁 | Docker 恢复后通过：Rust workspace 494 passed / 2 个真实渠道凭证探针按原配置 ignored，Web 397 项、Python 25 项及全部 SDK/Runner/模板检查通过 | `20260913-closure/xtask-full.log` |
| 正式镜像 | 11 个正式镜像及 2 个 Echo 镜像均构建完成；13 个节点镜像引用已核对 | `images.log`、`image-reference-verification.json` |
| 镜像导入 | 修正为 stdin 流式导入；通过 `cargo xtask images --values deploy/values/local.yaml --service echo-mcp` 验证 | `image-stream-import.log`、`image-import-check.log` |
| Linux musl 发布包 | release 构建、归档、SHA-256、仓库外 validate/render 通过 | `linux-release.log` |
| 历史 HTML 报告 | 实际 Chromium 打开并渲染成功 | `report-probe.log`、`historical-report.png` |
| 完整 Kubernetes E2E | Run `9a85fabeb4`：26 passed，0 failure/error/skip，1179 秒；包含 32 个 Playwright 场景，使用目录迁移后构建的镜像 | `20260913-closure/e2e-full.log`、`e2e.junit.xml`、`final-verification.json` |
| 首次实施环境核对（历史） | 当时 23 个 Agentx/Argus Deployment 副本保持、12 个 PVC UID 不变且 Bound；后续 Argus 清理采用新的明确授权 | `20260912-layout/environment-verification.json` |
| 最终环境核对 | 9 个正式 Agentx Deployment 恢复原副本且就绪；6 个 PVC UID 保持且 Bound；Web 经 Service 端口转发返回 200，Bootstrap API 返回 200/required=false；Sandbox、Argus/E2E 残留为零 | `20260913-closure/final-verification.json`、`formal-web-final.json`、`formal-api-final.log`、`opensandbox-after-e2e.json` |

## 验证中修复的问题

- 清理 target 后，acceptance 需要的独立 CLI 尚未生成；统一门禁已明确先构建 agentxctl 与 Backup Adapter，再运行 Python 验收。
- 将浏览器测试纳入行数门禁后发现既有 Studio 测试为 2,003 行；仅合并冗余空行至 1,999 行，图片基准未重录。
- 并行重建依赖和镜像时，Web 默认 worker 数造成若干测试超时；统一门禁使用 2 个 worker，全部断言不变，397 项测试通过。
- Runner 取消用例的测试插件未处理已经取消的 AbortSignal，可能在模块加载期间错过 abort 事件；测试 Fixture 增加 `throwIfAborted()` 并确保失败时清理子进程。继续立即发送取消并保留原有断言，Runner 生产代码未改变。
- Docker `cp` 在当前异常转发环境中返回成功但未写入节点归档；改为 `docker exec -i ... ctr images import -`，减少中间文件。host 归档由 TempPath 管理，错误返回时也会清理；使用新入口验证成功。
- Linux 验证镜像下载辅助工具时遇到外部 502/连接断开；从官方 URL 下载并核对 SHA-256 后作为独立 build context 提供，最终完成真实 musl 二进制及仓库外验证。

## 环境诊断

以下初次阻塞已在 2026-09-13 恢复，详见下节；保留记录用于解释首次完整门禁为何失败。

本机 Kubernetes 节点可访问，正式 Agentx/Argus 服务和 PVC 已保存执行前快照。Docker Engine 在 WSL 内直接访问正常，但 Windows 的两个默认 Docker 命名管道连接超时；MySQL 测试容器已打印 ready，而 Windows 访问其新映射端口也超时。

首次实施使用仅绑定 `127.0.0.1` 的临时 Docker API 转发进行镜像验证，没有修改 Docker 配置。它不能修复 Windows 的容器端口转发问题，因此当时没有宣告完整验收通过。首次实施产生的两个 MySQL 测试容器、临时转发、报告验证服务器、Service 端口转发及遗留镜像 TAR 已清理，证据见 `20260912-layout/test-container-cleanup.json` 与 `local-helper-cleanup.json`。2026-09-13 恢复 Docker 后，后续检查直接使用默认 Docker 连接，不再依赖该临时转发。

首次实施未运行完整 Kubernetes E2E；2026-09-13 已按追加授权恢复环境并完成全部后续验收，结果见下节。

Linux 严格 CNI/生产隔离认证不属于本次本机功能验证结果；未运行时必须继续标记未执行，不能由 Windows 测试推导通过。

## 2026-09-13 清理与恢复

用户明确授权清理 Kubernetes 中全部 Argus 部署和遗留 E2E 环境，并继续完成本计划验收。

- 清理 Argus 的三个 Namespace、六个专属 PVC/PV、九个应用/上游 Helm Release、17 个专属 CRD，以及其专属 PKI、RBAC 和 `kube-system` 镜像加载 DaemonSet。
- 核实没有其他使用方后，卸载 Argus 独占的 ingress-nginx、cert-manager、trust-manager 及其七个 CRD。正式 Agentx 的独立 IngressClass `agentx-nginx` 和控制器保留。
- 清理两个命名空间对象已缺失的历史 E2E 环境及五组遗留 IngressClass/RBAC。旧 LoadBalancer 清理因 Namespace 不存在被控制器拒绝；先删除残留工作负载，再临时恢复 Namespace 对象，让正常 Namespace/LoadBalancer 控制器完成删除，没有强制移除 finalizer。
- 清理后按 Namespace、工作负载、Service、PVC/PV、Ingress、RBAC、Webhook、CRD 复核，Argus/E2E 目标残留为零；仅保留正式 Agentx 的五个 Helm Release、六个原 UID 的 Bound PVC。证据为 `cleanup-summary.json` 与 `*.after-cleanup.json`。
- 本次清理范围为 Kubernetes；没有调用会同时删除 Docker 本地 registry 的 Argus 全宿主机卸载入口，也没有清理 Docker 镜像缓存。
- Docker Desktop 常规 restart 命令超时。重启桌面进程后，日志确认 `sailor-ingest.sock` 与 Secrets Engine 的 AF_UNIX 通信文件报 WinError 1920。只将包含已核实零字节通信文件的两个临时目录隔离并重建，未移动或删除集群磁盘、数据库、配置和密钥数据；随后从 Windows 桌面启动上下文重新启动 Docker Desktop。
- Docker API 恢复为 Engine 29.7.2，节点 Ready，正式 Agentx 六个 PVC UID 保持；原来失败的两个 MySQL Testcontainers 用例均通过，之后完整 `cargo xtask check` 全部通过。
- 已启动原有 Agentx OpenSandbox Docker 容器；主机认证列表为空，Runtime Pod 到其 `/health` 返回 `200 {"status":"healthy"}`。没有复用未知运行中的 Sandbox。
- 26 项完整 E2E 已使用 `pytest tests/e2e` 唯一编排入口和临时三 Namespace 全部通过。按既有 `--scale-down-development` 机制临时缩容后，正式应用副本已恢复，九个 Deployment 均就绪，六个正式 PVC 的 UID 与状态再次核对通过。
- Run `9a85fabeb4` 的三 Namespace、IngressClass、RBAC、Service 及其 PVC 已清理；当前仅剩正式 Agentx 和 Kubernetes 系统 Namespace。OpenSandbox 认证列表为空，测试 port-forward 已退出。
- 最终检查再次确认 SQL、OpenAPI、Schema、Spec、两份内置插件 runtime.js 和图片基准摘要均与迁移前一致，未产生新的截图基准；根文件仍为 16 个。

最终执行命令：

```bash
cargo xtask check
uv run --frozen --group test pytest tests/e2e --values deploy/values/local.yaml --scale-down-development -v --capture=tee-sys --junitxml=.local/artifacts/repository-layout/20260913-closure/e2e.junit.xml
```

完整仓库门禁包括 494 项 Rust workspace 测试、397 项 Web 测试、25 项 Python acceptance，以及 SDK/Runner/内置插件/模板检查。两个需要真实钉钉/飞书凭证的 Rust 探针保持原来的 ignored 设置；Linux 严格 CNI/生产隔离认证未运行，不属于本次本机验收结论。
