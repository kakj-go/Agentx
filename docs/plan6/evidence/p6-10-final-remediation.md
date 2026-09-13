# P6-10 严格复核整改与最终交付证据

状态：完成。

验收日期：2026-09-06。

## 1. 完成结论

P6-08 发现的实现与证据缺口已经全部关闭。当前完成度为 79/79 项实施任务、40/40 项 E2E 场景。最终链路已经打通：

`下载模板 → 仓库外开发 → check/test/build/pack → 页面导入 → 画布配置 → 保存重开 → 调试/发布 → Node.js 执行 → 下游引用 → Trace 展示 → 版本管理`

本次采用破坏性切换，没有增加旧数据迁移、字段别名、双读、旧执行路径或兼容包装。正式本地安装在验收后清除了 `agentx-control`、`agentx-runtime`、`agentx-deps` 的旧数据并重新安装；`argus-*` 和共享基础设施未被清理。

## 2. 严格复核关闭内容

- Workflow、子 Workflow、Draft Debug、生产 Deployment 和 Evaluation 使用完整插件依赖闭包；每个子 Workflow 使用自身解析并冻结的 Manifest/IR。
- 插件 Runtime 源码先写入 Runtime OSS，Work Package 只携带不可变对象引用；Worker 使用有界本地缓存，逐次校验摘要和大小，损坏缓存会按同一摘要重新获取。
- 设计时 Provider 通过 Runtime Runner 执行，支持 HTTP、Model、Credential 和 Artifact 宿主资源；资源来自 Control 冻结快照，缺失与权限错误有稳定结果。
- `agentx/core`、`agentx/data`、`agentx/http` 使用正式包 Manifest。Set、List、HTTP 使用公开 TypeScript SDK；核心节点保留 Rust 原生能力并统一 UI 注册入口。
- Runner 支持双向 JSON-RPC、分片传输、stdout 污染隔离、超时/取消、崩溃回收和进程树治理。OpenSandbox stdio MCP 在创建 PTY 前执行有界 execd readiness 探测，消除容器启动竞争。
- 插件 Trace 支持零埋点 Node/Attempt、实时子 Span、版本化多内容、Artifact 大内容、历史 renderer、缺失 renderer 标准回退和成本归属。
- 模板包含根 `AGENTS.md`、字段级协议、vendored SDK/Runner 和真实 `dev/check/test/build/pack:plugin`；独立目录变体通过实际页面导入执行。
- 画布连接 E2E 使用真实 handle 拖拽；在布局命中失败时通过可见自动布局与适应画布重新定位后重试，不使用内部 Store 或 API 伪造连线。

## 3. 自动化结果

| 检查 | 结果 |
|---|---|
| `cargo xtask check` | 通过；Rust workspace、Runtime Slice、Clippy/格式、前端 lint/test/build、SDK/Runner/模板、契约漂移、文件边界和 Helm 检查均成功 |
| `pytest tests/e2e --values deploy/values/local.yaml --scale-down-development` | Run `2e93929769`，26 passed，1190.53s |
| Playwright | Run `2e93929769` 下 14 个 JUnit 套件、32 个用例，0 failure、0 error、0 skip |
| 独立模板变体聚焦 | 1 passed，302.41s；下载 ZIP 后在仓库外修改、安装、检查、测试、构建、重复打包和页面导入执行 |
| Model Trace 聚焦 | 1 passed，354.64s；Runtime Query 短暂不可用重试及大响应 Artifact 内容最终一致性通过 |
| stdio MCP/OpenSandbox 聚焦 | 与 Model 聚焦共享环境通过；完整 Run `2e93929769` 中再次通过 |
| 正式安装后插件主链路 | `formal/plan6-final-retry/canvas-plugins`，1 passed，28.5s |
| E2E TypeScript | `pnpm --filter @agentx/e2e exec tsc --noEmit` 通过 |
| Git whitespace | `git diff --check` 通过；仅输出 Windows LF/CRLF 提示 |

完整系统证据位于 `artifacts/e2e/2e93929769/` 和 `apps/e2e/test-results/helm-agentxctl/2e93929769/`。正式安装证据位于 `apps/e2e/test-results/formal/plan6-final-retry/canvas-plugins/`，正式执行 ID 为 `01a076ac-8ca8-7450-bded-5360a2989fb9`。

## 4. 性能证据

性能文件：`apps/e2e/test-results/helm-agentxctl/2e93929769/workflow-performance/artifacts/workflow-canvas-performanc-feebb-in-node-interaction-budgets/workflow-canvas-performance.json`。

| 节点数 | 中位 FPS | p95 输入延迟 | 结果 |
|---:|---:|---:|---|
| 100 | 59.9 | 28.7ms | 通过 |
| 300 | 59.9 | 28.4ms | 通过 |
| 500 | 59.9 | 21.8ms | 通过，满足 FPS ≥ 50、延迟 ≤ 50ms |
| 1000 | 59.9 | 24.9ms | 通过，满足 FPS ≥ 30、延迟 ≤ 100ms |

测试同时覆盖插件面板反复挂载/卸载、CSS 引用清理、虚拟化节点数量和缓存回收。

## 5. 最终制品

模板包：`templates/canvas-plugin/dist/acme-json-mapper.agentx-plugin`，SHA-256 `abfdec7115cc24026baaee5ceb157c240157177f0753561e5e1d8c33832b611b`。

| 镜像 | 本地不可变 ID |
|---|---|
| `agentx/platform-control:dev` | `sha256:cf72a6b7429eaa8e01a41bd1a521260bdb0d1f5d12a17df20bd77035f2b6c248` |
| `agentx/web-console:dev` | `sha256:32e5ee76ad04cb984b4abffc8ae67bc94a95ed0bbfe7112b764337f97be3cf1b` |
| `agentx/workflow-worker:dev` | `sha256:52b9311ce43ab71799a72a6bc5d3072879be5a8d0c92f14407eed5aedd2dd844` |
| `agentx/sandbox-manager:dev` | `sha256:cb57ccacfce72aa22f513a8c8c186a56c42c5680d10581baeff74d2fdd3d78ad` |
| `agentx/runtime-gateway:dev` | `sha256:41cb8fbec47b049020ea9ba60e94da768ac384b91027641de3f632a40e051ee1` |
| `agentx/workflow-runtime:dev` | `sha256:3bd725b3198a160261de267f3bd93e01c6227c66cdbee7b2bb23d823cbc9cb49` |
| `agentx/observability:dev` | `sha256:cbb8b272bea620a89e2e9c2eaae6ea338b3085c7041e044bf5326ccc5e83b3e9` |

其余部署镜像 ID 已随正式安装 receipt 和本轮命令输出记录。Worker 内 Node 版本为 `v24.20.0`；`AGENTX_PLUGIN_CACHE_DIR=/var/run/agentx/plugin-cache`，emptyDir 挂载可写且受 Helm 资源预算约束。

## 6. 正式安装与清理

- `agentxctl uninstall --target all --purge-data --yes` 返回 `status: purged`，范围仅为三个 Agentx namespace。
- `agentxctl install` 返回 `status: ready`，Dependencies、Control、Runtime、Observability release 均为 revision 1。
- `platform-control`、`web-console`、`runtime-gateway`、`workflow-runtime`、`workflow-worker`、`sandbox-manager`、`observability` 和 `agentx-egress-gateway` 最终均为 1/1。
- 本机端口 80 由另一套本地入口占用，正式页面验收使用临时 port-forward 访问 Web 和 Runtime；验收后两个 port-forward 均已关闭。
- 最终不存在 `agentx-e2e-*` namespace、E2E Admission Webhook、遗留插件/Sandbox 容器或 18081/18082 监听端口；OpenSandbox 常驻服务保留并保持健康。

## 7. 外部边界

当前正式产品范围是 Web 桌面端和可信审批插件。真实第三方飞书/钉钉凭据探测未运行，Rust workspace 中对应 2 项测试按凭据前提 ignored；平台协议、会话和消息行为由本地确定性测试覆盖。Model、MCP、HTTP、RAG、Memory 和 Code 外部交互使用仓库固定 Fixture，所有 plan6 要求的本地与 Kubernetes 场景均已执行。

本轮没有创建 Git commit；验收基于当前工作树，基线提交为 `e774d272e3fff2b3432e4ad856654d0f1ee68691`。
