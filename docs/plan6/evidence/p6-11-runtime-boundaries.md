# P6-11：插件执行边界修复与验收

日期：2026-09-13。状态：本轮修复及插件 Kubernetes 验收完成；完整仓库门禁的 Docker 环境限制单列于下方。

## 修复范围

1. **模块内存与调用隔离。** Worker 为每次 execute、Runtime Gateway 为每次设计时调用创建独立 Node 进程和工作目录，完成、失败、超时或取消后回收整个进程树。删除空闲 Runner 复用及 `pluginIdleSeconds`。继续复用按摘要验证的 Runtime 源码缓存，不再用不同 import URL 在长寿命 ESM 环境中积累模块。
2. **真实并发。** `pluginMaxProcesses` 同时控制 Worker 的插件消费循环和 Node 进程容量。每个循环使用独立 Redis 连接与 consumer，按一条任务读取；容量随进程退出释放，等待者能够及时继续运行。
3. **动态契约。** 画布通过 `POST /api/v1/workflows/{id}/draft/resolve-plugins` 提交当前 Definition，复用后端保存使用的解析顺序与上游契约。Start、Contexts、节点契约和多级动态端口使用同一来源。前端对影响节点解析的图输入及包摘要建立查询键，并取消旧请求、抑制晚到结果。结束契约编辑不清空已解析节点；按不可变 UI 制品身份保持 Canvas 局部状态，字段对话框在异步契约更新时保留未提交输入。
4. **Trace 降级。** SDK 在数量、深度、内容大小、非法 JSON 或 stdout 背压超预算时丢弃诊断，继续执行 Span 内业务。Worker 使用有界事件队列和独立诊断通道写入原有 Outbox；不在业务 deadline 内等待诊断落库。诊断缺失通过 `plugin.trace.incomplete` 和 `traceIncomplete/droppedDiagnostics` 标记。详情查询随已有 ingestedWatermark 和 Span 生命周期更新，避免缓存过早取得的内容；同一 Span 刷新保留选中标签，并立即回收旧水位缓存。
5. **文件桥接。** SDK API 2 提供 `artifacts.read(ArtifactRef)` 与 `artifacts.put({path,fileName,contentType})`。文件通过调用目录和 ObjectStore 流传递，不进入 RPC Base64 帧。单文件 64 MiB、每次调用累计传输 128 MiB；检查租户、Execution 与输入引用授权、相对路径、符号链接、大小和 SHA-256。设计时文件只保存在调用目录。多段上传在调用取消时执行 abort。

包格式仍为 1；SDK API 与 Runner RPC 升为 2，拒绝旧 SDK/RPC，不保留兼容入口或数据迁移。模板包含 Node 类型声明配置、公开 SDK 生成的 vendored 声明以及文件流示例。

Runtime Gateway 与 Workflow Worker 均挂载专用 emptyDir，并以 `fsGroup: 65532` 保证非 root 进程可写。Runtime Coordinator 不承载该文件目录。三套 Values 的渲染验收覆盖目录、卷、权限与只读根文件系统。

## 验证结果

| 检查 | 结果 |
|---|---|
| Node Runner | 12/12；包含 120 次带模块局部数组的独立调用、单次堆内存低于 32 MiB、拒绝同进程第二次调用、各类 Trace 预算降级 |
| Rust 插件与文件桥接 | 9/9；包含 10 MiB 流式往返、未授权/伪造引用拒绝、调用目录清理、2 槽并发和等待者唤醒 |
| Domain/Node Protocol/Runtime/Runtime Contracts | 133 项通过 |
| Bundle Builder | 13/13，包括子 Workflow 的独立动态契约闭包 |
| Web | 90 个文件、403 项通过；lint 无错误、生产构建和 E2E TypeScript 通过 |
| SDK、Set/List、HTTP、模板 | SDK 3、Data 3、HTTP 1、模板 1 项通过；独立模板类型检查通过 |
| Python acceptance | 全量 25 项通过；随后 Gateway 挂载/权限断言所在部署检查再次 7/7 通过 |
| Rust Clippy、边界与格式 | Clippy workspace/all-targets 通过；生产边界检查与相关格式检查通过 |
| 插件 Kubernetes E2E | 最终 Run `8f7b574e7a`，9/9，通过，362.27 秒；此前 `f2dd0397a2` 的聚焦 8/8 亦通过 |
| 原有发布/版本/历史 Trace 与 ClickHouse 降级 | 最终 Run 中通过；5 个 Playwright JUnit 套件、5 个用例全部通过 |

聚焦 Kubernetes Run 包含页面导入、真实画布连线、上游字段变化后下游端口更新、保存重开和版本编译；10 MiB 文件跨节点读取且 Hash 一致；伪造 Artifact 与目录逃逸拒绝；快任务先于同时运行的慢任务完成；100 个 Span 内业务全部执行且保留诊断缺失标记；Runner 取消/崩溃/后台进程与版本握手；保留期清理；在仓库外下载、修改、构建、打包和页面执行模板变体。

本机 120 次独立 Runner 调用约 4.9 秒。进程启动成本已纳入测试；没有声称冷调用零开销，也没有进行生产容量压测。

## 实际发现与修正

- 首轮静态检查发现新卷与空 CA 挂载列表拼接错误，已修正条件渲染。
- Run `7dd557a3b0` 发现设计时调用位于 Runtime Gateway，需要在那里提供文件目录；同时独立模板需要显式启用 Node 类型。修正后 Run `f2dd0397a2` 全部通过。
- 原有发布回归还暴露输出字段对话框随父级刷新重置的问题，已稳定其编辑状态和插件 Canvas 的挂载依赖，并增加三项组件回归。
- 后续原有发布回归确认 Trace 详情曾缓存结束前的两份内容，未加载随后到达的表格；已接入水位驱动的详情刷新并增加回归。
- 工作树原有 `docs/todolist.md` 修改始终保留。本次未提交，也未升级正式 Deployment 的镜像。

## 环境与验证边界

Docker Desktop 的 container list/inspect API 在本轮期间持续超时，另一个 Argus 容器查询也受影响；版本 API 与 Kubernetes `/readyz` 正常。完整 `cargo xtask check` 在两个已有 MySQL Testcontainers 测试的 `WaitContainer(StartupTimeout)` 处停止，不能标记为全量通过。是否重启共享 Docker 已单独询问用户，本轮未擅自重启。

镜像采用独立标签 `plugin-fix-20260913-171249`，通过仓库现有 Kubernetes loader 导入。首个 Run `23869975fd` 因 Docker Desktop 的 LoadBalancer 分配停滞而安装超时；该 Run 的服务没有分配地址，也没有监听对应宿主端口，清理了其残留 finalizer。后续仅在本地测试 Values 中使用仓库支持的 NodePort 模式，不改正式部署入口。所有测试仍从 `pytest tests/e2e` 编排。

最终 Run `8f7b574e7a` 的临时 Namespace、Loader Pod、端口转发和调用目录均已清理；Control、Runtime、Dependencies 的全部 Deployment 已恢复原副本并 Ready，正式 PVC 均为 Bound。未升级正式镜像，未清理正式数据。Docker 容器级残留因 container API 故障无法复核，不能扩大为 Docker 全局清理证明。

证据入口：`.local/artifacts/workflow-plugin-review-20260913/`，包括命令日志、镜像上下文、源码 Hash 和失败环境记录；系统记录位于 `.local/artifacts/e2e/<run-id>/`，页面证据位于 `.local/artifacts/playwright/helm-agentxctl/<run-id>/`。
