# SDK、开发模板与 AI 开发指南

状态：部分完成。模板和命令已交付并可在独立目录构建；同源类型生成、仓库外变体开发及该产物的页面导入仍待完成。

## 1. SDK 包边界

pnpm workspace 已包含以下包：

| 包/目录 | 职责 |
|---|---|
| `src/plugins/packages/plugin-sdk` | 服务端类型、defineNode、Items、资源/Trace Context、协议 helper |
| `src/plugins/packages/plugin-ui` | 浏览器组件/编辑接口与共享依赖注入契约 |
| `src/plugins/packages/plugin-runner` | 平台维护的 JSON-RPC 服务端、模块加载与调用清理 |
| `packages/plugin-devtools` | UI 预览、正式 Runner 测试、构建和协议检查 |
| `src/plugins/builtin/*` | agentx/core、agentx/data、agentx/http 的正式节点包 |
| `src/plugins/templates/canvas-plugin` | 可独立下载和运行的开发模板 |

服务端 SDK 不导入 React；UI SDK 不导入 node:* 或凭据执行代码。核心 Rust 生成的协议类型通过单一生成管线提供，SDK 不能手写并行 DTO。包名和 workspace 发布策略在 P6-00 固定。

不把内部 Feature 文件、Agent Core 或 agentxctl 作为 SDK 私有依赖。SDK 的正式发布可以是内部 npm registry 或随模板附版本化包，必须选择并验证一个可用分发方式；模板不得依赖 Agentx 仓库相对路径才能安装。

本期默认交付自包含模板 ZIP：包含通过同一流水线生成的 SDK/devtools npm tarball，并使用相对 `file:vendor/*.tgz` 锁定；其公开第三方依赖仍由锁文件解析。这样不依赖尚未存在的公网或私有 npm 服务。平台内开发复用同源码 workspace，生成模板时打包正式产物；不另写一套 SDK。

## 2. 模板目录

```text
canvas-plugin/
  AGENTS.md
  README.md
  package.json
  pnpm-lock.yaml
  tsconfig.json
  vendor/                    固定版本 SDK/devtools 包
  src/
    node.ts                  节点定义与默认值
    contract.ts              可选的纯契约解析
    runtime/execute.ts
    runtime/providers.ts
    ui/panel.tsx
    ui/canvas.tsx
    ui/result.tsx
    ui/trace-view.tsx
  assets/
  docs/
    node-contract.md
    ui-sdk.md
    runtime-sdk.md
    trace.md
    testing.md
  examples/
    input.json
    config.json
    output.json
  tests/
    contract.test.ts
    runtime.test.ts
    panel.test.tsx
```

模板选一个“JSON 字段映射”完整例子，不依赖真实网络或模型即可执行。另交付“HTTP 客户查询 + 业务 Trace 表格”示例，覆盖设计时字段搜索、凭据/HTTP bridge 和自定义内容。示例接口使用 E2E Fixture；不得附带真实密钥。

## 3. AGENTS.md 必须包含的内容

根文件按以下顺序组织，使用可直接执行的实际命令与本地链接；所有引用都随模板交付：

1. 本工程的目标、SDK/API/Node/包管理器版本、正式支持能力及目录结构。
2. 开始修改前阅读 `docs/node-contract.md`，再按 UI/执行/Trace 阅读对应文档；先跑原样例检查环境。
3. 描述需求如何映射到 node.ts、panel.tsx、execute.ts、Trace renderer 和测试，不让 AI 猜私有扩展点。
4. 规定稳定节点 ID、包版本、节点 typeVersion、端口、Schema、InputBinding、Item/Lineage/Artifact 的具体语义。
5. 解释解析前的配置与执行时已解析参数差别，给出数组/对象、逐 Item 参数、多端口与空输出例子。
6. 规定使用公共组件、主题、SmartInput、编辑事务与只读 Context，不导入宿主内部路径。
7. 解释 `resolveDefinition` 的确定性与 incomplete/invalid/complete；Provider 的搜索取消和资源作用域。
8. 解释 SDK HTTP/Model/Artifact 调用、deadline、取消、外部副作用、幂等及 Outcome Unknown。
9. 说明自动 Trace、可选子 Span、内容 Schema/版本和展示；不要为已有平台 Model Span 重复报告成本。
10. 明确 stdout 是协议、console 由 Runner 处理；不实现自定义 RPC server，不修改 Runner，不做持久后台进程。
11. 列出 dev/check/test/build/pack 命令、产物位置、成功标准和如何读取字段错误/source map。
12. 交付时报告变更、契约变化、通过的检查、尚未验证的宿主/网络条件；不要在本地预览后声称 Kubernetes 已通过。

AGENTS.md 负责导航和工作约定，字段级协议放在 docs 与生成的 .d.ts/Schema。禁止将整个协议手抄进多份文档后独立维护；版本更新必须同时再生成模板与 conformance 结果。

## 4. 字段级文档要求

每个公开方法列出参数、必填性、类型、默认行为、返回值、错误码、取消/超时和执行副作用。至少给出：

- 一个简单节点和一个有自定义 UI/Provider/Trace 的完整节点。
- `execute` 成功、确定性失败、可重试基础设施失败和未知外部结果。
- 输入字段缺失、空数组、多个 Item、Lineage 合并、ArtifactRef。
- UI patch 的原子操作、只读状态、Reference Picker 与错误路径对齐。
- 包安装、版本更新、停用、缺包、SDK 不匹配对开发与运行的影响。
- TypeScript 代码例子与真实 Runner 夹具；协议 JSON 请求响应同时给出。

协议范例不需要再提供 Python/Java SDK；用户插件只支持 TypeScript/Node.js。公共平台管理 API 的 OpenAPI 仍按项目现有文档规范生成。

## 5. 开发命令与离线边界

模板的 `package.json` 必须实际提供以下脚本，统一用 pnpm；名称在交付时不能停留在文档占位：

| 命令 | 必须完成 |
|---|---|
| `pnpm install --frozen-lockfile` | 从模板附带 SDK 包和锁文件建立依赖，无 monorepo 路径 |
| `pnpm dev` | 用真实 UI SDK 预览节点、参数、只读结果、Trace 内容 |
| `pnpm check` | TS/lint、Manifest、Schema、依赖与入口检查 |
| `pnpm test` | 正式 Runner 协议测试、业务用例、组件交互 |
| `pnpm build` | 生成独立浏览器 ESM/CSS 与服务端 JS/Manifest |
| `pnpm pack:plugin` | 复核构建结果，生成 `.agentx-plugin` 和校验报告 |

模板自包含的是 Agentx SDK，不承诺所有第三方 npm 依赖已经离线缓存；断网环境需要用户预备包管理器镜像/缓存。插件正式安装与执行本身不依赖 npm 网络。

Node 生态构建、测试和插件工具允许 TypeScript；仓库跨平台部署、临时验证与 Kubernetes 编排继续使用 Python，并调用 agentxctl/xtask。不能新增 PowerShell 包装脚本。

## 6. 预览与 conformance

UI 预览使用宿主同一组件和设计令牌，支持主题/语言、假定的可达上游契约、字段错误、只读状态、模拟 Provider 延迟/失败。明确标注模拟环境；预览不能代替后端引用校验。

Runner 测试使用正式 plugin-runner 产物，输入来自 examples，不把 execute 直接 import 后单元测试作为唯一运行证明。测试必须校验 Schema、端口、Lineage、异常、取消、JSON-RPC 分帧和 Trace 内容。

`pack:plugin` 检查：

- 稳定身份、SDK 版本、前后端入口存在且导出合法。
- 所有资源路径可解析，Hash 无循环，自定义 Trace 类型有 Schema。
- UI 不重复打包 React，无服务端依赖；运行制品不存在未声明安装依赖。
- 纯 resolveDefinition 在相同输入下输出稳定，跨两次独立 Runner 结果一致。
- 无原生 npm 扩展或系统可执行依赖；不把 TS 能编译误判为 Node 环境可运行。
- 默认样例通过真实 Runner，输出/诊断符合契约，包可以被平台导入校验器接受。

## 7. AI 开发体验验收

从导出的模板 ZIP 解压到 Agentx 仓库之外的全新目录，只提供该目录的 AGENTS.md、用户业务要求和公开 SDK：

1. 将样例改成字段重命名/过滤插件，增加一个面板交互和一个自定义 Trace 内容。
2. 运行模板命令，生成包；测试不能读取宿主内部文件。
3. 通过真实“画布插件”页面导入，在 Workflow 中完成执行并查看自定义 Trace。
4. 保存开发输入、关键修改、构建报告和产品 E2E 证据，核对文档是否遗漏协议。

人工或 AI 都可以执行这次开发实验；它是开发体验证据，不是替代自动化门禁。流水线仍要自动验证模板原样例、至少一个变体和全部本地链接，避免只对一次 AI 生成结果有效。
