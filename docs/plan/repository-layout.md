# 仓库目录归并计划

状态：已完成（2026-09-13）。完整仓库门禁和 26 项 Windows Kubernetes E2E 均通过；用户追加要求的 Argus/历史 E2E 清理也已完成。完成事实与验证边界见 [验收记录](repository-layout-evidence.md)。

## 1. 目标与约束

将 15 个主要工程目录归并为 `src`、`contracts`、`deploy`、`tools`、`tests`、`docs` 六个目录。将 `todolist.md` 移入 docs，删除 `skills-lock.json`，根文件从 18 个减少到 16 个。保留 `.agents/skills`。

这是仓库文件组织调整，不修改三平面职责、业务行为、公共 API、Schema ID、协议版本、npm/Cargo 包名、二进制名、镜像名或数据库内容。保持原来的 24 个 Cargo 成员和 8 个 npm 子包。不增加旧路径兼容入口、软链接、迁移框架或第三方依赖。

当前工作树的已修改与未跟踪源码均属于实施基线，不能从 HEAD 覆盖。禁止自动 stash、reset、git clean、暂存、提交、推送和发布。

## 2. 最终结构

```text
Agentx/
├── src/
│   ├── web/
│   ├── services/
│   │   ├── platform-control/
│   │   ├── agentx-v2-runtime/
│   │   ├── agentx-egress-gateway/
│   │   └── observability/
│   ├── crates/
│   │   ├── agentx-agent-core/
│   │   ├── agentx-api-types/
│   │   ├── agentx-application/
│   │   ├── agentx-bundle-builder/
│   │   ├── agentx-control-infrastructure/
│   │   ├── agentx-domain/
│   │   ├── agentx-key-material/
│   │   ├── agentx-mysql-lease/
│   │   ├── agentx-node-protocol/
│   │   ├── agentx-runtime/
│   │   ├── agentx-runtime-contracts/
│   │   ├── agentx-runtime-infrastructure/
│   │   ├── agentx-service-kit/
│   │   └── agentx-v2-ops/
│   └── plugins/
│       ├── packages/{plugin-sdk,plugin-ui,plugin-runner}/
│       ├── builtin/{core,data,http}/
│       └── templates/canvas-plugin/
├── contracts/
│   ├── openapi/
│   ├── schemas/{agent-core-v1,runtime-v1}/
│   └── vendor/opensandbox/specs/
├── deploy/
│   ├── docker/
│   ├── helm/
│   ├── ingress-nginx/
│   ├── kustomize/
│   ├── opensandbox/
│   ├── values/
│   ├── release/
│   └── migrations/{control,runtime,observability}/
├── tools/
│   ├── agentxctl/
│   ├── agentx-boundary-check/
│   ├── xtask/
│   └── scripts/{dev,plan3,release}/
├── tests/
│   ├── acceptance/
│   ├── e2e/
│   ├── browser/
│   └── fixtures/{backup-adapter,echo-mcp,echo-node}/
├── docs/
│   ├── README.md
│   ├── plan/repository-layout.md
│   ├── plan/repository-layout-evidence.md
│   └── todolist.md
├── .agents/
├── .cargo/
├── .github/
├── README.md
├── AGENTS.md
├── LICENSE
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── package.json
├── pnpm-workspace.yaml
├── pnpm-lock.yaml
├── pyproject.toml
├── uv.lock
├── .python-version
├── .gitignore
├── .gitattributes
├── .dockerignore
└── .editorconfig
```

docs 的正式架构文档、参考资料、图片与历史阶段目录保持现有分类。源码内部的单元测试、Rust 集成测试、插件包自身测试继续与所属模块放在一起。

## 3. 路径映射

| 原路径 | 新路径 |
|---|---|
| `apps/web` | `src/web` |
| `apps/e2e` | `tests/browser` |
| `crates/agentxctl` | `tools/agentxctl` |
| `crates/agentx-boundary-check` | `tools/agentx-boundary-check` |
| 其他有效 `crates/*` | `src/crates/*` |
| `services/echo-mcp`、`services/echo-node` | `tests/fixtures/echo-mcp`、`tests/fixtures/echo-node` |
| 其他有效 `services/*` | `src/services/*` |
| `packages` | `src/plugins/packages` |
| `plugins/builtin` | `src/plugins/builtin` |
| `templates` | `src/plugins/templates` |
| `openapi`、`schemas`、`vendor` | `contracts/openapi`、`contracts/schemas`、`contracts/vendor` |
| `migrations/{control,runtime,observability}` | `deploy/migrations/{control,runtime,observability}` |
| `scripts`、`xtask` | `tools/scripts`、`tools/xtask` |
| `todolist.md` | `docs/todolist.md` |
| `skills-lock.json` | 删除 |

先处理工具与 Echo 子目录，再搬迁父目录。删除已核实为空的旧服务、旧 Crate、`migrations/mysql`、`deploy/k8s`、`deploy/profiles`；删除不再被执行的 `migrations/clickhouse` 三份旧 SQL。现行三个数据域的 SQL 保持字节不变。

历史证据中的原始源码路径、命令、Run ID、日志和测试结论是当时记录，不批量改写；按此表定位现在的源码。仍被程序读取的 `docs/planv2/contracts` 路径字段则必须更新。

## 4. 本地产物

用户选择清理可再生成产物、保留测试证据。

- 原 `artifacts/*` 证据按原相对层次迁入 `.local/artifacts/*`。
- 原 `apps/e2e/test-results/*` 迁入 `.local/artifacts/playwright/*`，保留 HTML 与附件关系、截图、Trace、视频和日志内容。
- 新 Python E2E 证据写入 `.local/artifacts/e2e/<run-id>`；新浏览器证据写入 `.local/artifacts/playwright/<stage>/<run-id>/<suite>`。
- CLI 发布包统一写入 `.local/dist`；镜像导入临时归档写入 `.local/tmp/images`。
- pytest、Ruff 缓存分别写入 `.local/cache/pytest`、`.local/cache/ruff`。
- 清理根 dist、旧本地发布包、镜像 TAR、旧 target、失效 node_modules、未纳入源码的包内 dist 与 TypeScript 增量缓存。保留发布验证日志。
- 内置插件两份 `dist/runtime.js`、模板 SDK 声明、源码 Fixture 和图片基准属于源码基线，必须保留。
- 保留 `.venv`、现有 `.local` 部署状态和 OpenSandbox 运行资料，不清理全局缓存、Docker 数据或 Kubernetes PVC。

迁移前后按 SHA-256 核对源码与证据。目标冲突时相同内容可合并，不同内容停止该项搬迁；禁止覆盖。递归操作前核实目标绝对路径位于仓库内，并避免跟随目录链接越界。

`agentxctl` 仓库外用户的默认 `artifacts/*` 输出不变。仓库 Backup/Restore 测试通过既有 `--artifact-dir` 写入本次运行目录，不添加环境探测或路径 fallback。

## 5. 实施顺序

1. 保存 Git 状态、暂存/未暂存补丁、全部现有源码归档、未跟踪文件及摘要；保存集群副本、PVC 与入口基线。
2. 迁移和校验历史证据，清理明确的可再生成产物；执行源码映射，核对所有文件去向与摘要。
3. 更新 Cargo members/path、pnpm importer/link 和工作区目录；保持依赖版本与 `cargo xtask`、`pytest tests/e2e` 入口不变。
4. 修复 `include_str!`、`include_dir!`、`include_bytes!`、SQLx、build.rs、类型生成、模板下载、Runner、Fixture 和权限扫描路径；更新 SQL LF 属性。
5. 修复边界/行数检查器及机器输入；严格检查生产扫描入口存在且非空。最小 Fixture 按其成员检查，不将测试反例当作生产源码。
6. 更新 Docker COPY 与忽略规则、CI 发布输入；用仓库 Python 脚本复用打包、校验和仓库外 smoke。发布文件名、包内五文件约定与容器内路径保持不变。
7. 更新 Playwright 根路径和报告、Python 调用、缓存及文档。现有截图整体移动，不重录掩盖差异。
8. 完成静态、单元、契约、包、镜像和完整 Kubernetes 验收；验证环境恢复并写入验收记录。

## 6. 验收要求

| 门禁 | 要求 |
|---|---|
| 文件与布局 | 原始源码全部有去向；六个主要目录、16 个根文件；无旧目录和 skills-lock |
| 工作区 | Cargo 24 个成员、npm 8 个子包及名称不变；冻结安装、uv 锁文件检查成功 |
| 架构检查 | 新路径正例通过、违规反例失败、目录缺失不静默通过；保持 2000 行限制 |
| 契约 | Schema、OpenAPI、SDK 声明、Studio Catalog 无非预期漂移；SQL 摘要一致 |
| 本地 | cargo xtask check；Web、浏览器 TS、SDK、Runner、内置插件与模板测试/构建 |
| 部署 | 三组 Values、四 Chart、Addon/E2E Kustomize 渲染通过 |
| 镜像 | 新目录构建全部正式镜像与 Echo Fixture；Worker 内 Runner 资源完整 |
| 发布 | Windows 与 Linux 独立构建；解压后仓库外 validate/render；名称、五文件、SHA-256 一致 |
| 系统 E2E | 新镜像完整 Workflow、插件模板仓库外开发与导入、Worker 执行、Trace 闭环 |
| 证据和清理 | 历史证据摘要一致；临时 Namespace/进程/容器清理；开发副本和正式入口恢复 |

```bash
cargo xtask check
uv run --frozen --group test pytest tests/e2e --values deploy/values/local.yaml --scale-down-development
```

Windows 功能 E2E 和 Docker Linux 构建属于本次必跑项。Linux 严格 CNI/生产隔离认证保留现有 CI 门禁，若本次未运行必须明确记录。测试只清理本轮临时资源，不删除正式 PVC，也不为目录调整重置数据库。

本次不推送镜像、不打发布标签、不发布 Release、不自动提交。最终验收必须如实区分通过、失败、未执行及其原因。
