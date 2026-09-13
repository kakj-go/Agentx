# 动态 UI SDK 与内置节点抽取

状态：部分完成。动态 ESM/CSS 基础入口可用；动态契约、完整 Host Context、只读模式和真实 core 包仍待完成。

## 1. UI 扩展契约

| 扩展点 | 插件提供 | 宿主负责 |
|---|---|---|
| `panel` | 配置内容、复杂编辑器、业务校验提示 | 标题、调试入口、保存、字段错误、只读与撤销 |
| `canvas` | 节点内部内容、摘要、轻量交互 | 选择、拖动、端口、连线、尺寸与运行高亮 |
| `result` | 业务输入输出展示 | 执行查询、标准数据视图、Artifact 下载 |
| `traceRenderers` | 已声明内容类型的业务展示 | Span 树、时间轴、状态、分页、原始事件 |

`panel` 必须存在；canvas/result/traceRenderers 可选。未声明可选扩展表示作者选择平台标准展示，这是正式能力，不是旧协议兼容层。声明了入口但加载失败时显示明确错误，不假装该入口未声明。

画布插件不能通过 React Flow 实例任意重建图，也不能直接访问 Zustand 内部 store。自定义端口布局通过宿主提供的 Port 组件和已解析端口契约完成；端口 ID、方向和可连性由宿主校验。

## 2. 装载与构建

采用编译后的 ESM、原生动态 import 和同页面 React 挂载，不引入 iframe、Module Federation 或第二套页面框架。

- 宿主从授权 Catalog/执行快照解析 URL；URL 包含不可变摘要，同一制品稳定缓存。
- 浏览器加载已构建 JS，不在浏览器编译 TSX，不把源码字符串交给 eval。
- SDK 构建模板将组件制品导出为 `createUi(hostSdk)` 工厂；React、ReactDOM/JSX runtime 和公共 UI 能力通过工厂参数共享，私有依赖打入插件制品。最终浏览器入口不得残留未解析的 npm bare import。
- P6-01 必须用真实构建产物证明 Hooks、Context、Portal、Suspense、主题和同一 React 实例正确；不能只在 monorepo 开发服务器里证明。
- React 和 SDK 运行副本不得重复打包；前端 SDK 内部如何重写外部依赖只在统一构建工具实现，插件作者不维护自己的宿主注入代码。
- CSS 在插件构建时生成；使用 CSS Modules/局部命名和平台设计令牌，不能依赖宿主 Tailwind 扫描未来上传文件。禁止带全局 reset/preflight 重置整个 Portal。
- CSS、图标等资产与 JS 按相同摘要路径解析；挂载错误由插件级 ErrorBoundary 展示。ErrorBoundary 不能解决同步死循环，可信插件仍须通过预览与性能门禁。
- UI 使用当前权限和只读快照；不在 JSX 包里携带凭据、Node 服务端依赖或执行源代码。

按节点 type/version/包摘要注册组件；宿主缓存稳定组件引用，不在每次 render 新建 React Flow nodeTypes 对象。较重图表/编辑器只在面板或结果展开时加载。

卸载组件时清理 DOM、订阅与事件，不承诺浏览器能够卸载已导入的 ESM 模块。禁止通过时间戳 URL 无限绕过模块缓存；同摘要使用同 URL，宿主 SDK 更新通过正常页面更新流程生效。

## 3. 公共 UI SDK

建议新增 `src/plugins/packages/plugin-ui`，从现有 shared/ui 与 Workflow 公共能力提取稳定出口。扩展 pnpm workspace 以容纳 `packages/*` 和正式内置节点包；公共 SDK 不反向依赖 Feature 内部。

| 能力 | 最小接口职责 |
|---|---|
| 组件 | Button/Input/Select/Tabs/Field/SmartInput/ArtifactView 等，同一视觉体系 |
| 参数 | 读取当前配置，以路径或原子 patch 更新；返回稳定字段路径错误 |
| 编辑事务 | begin/commit 或等价单一事务接口；拖拽映射等复杂操作只生成一个撤销步骤 |
| 变量目录 | 带目标节点身份的 Reference Picker、InputBinding 编解码、上游契约 |
| 资源 | 现有资源选择和可用/授权状态；输出稳定资源 ID，不直接返回 Secret |
| 设计时调用 | invokeProvider/resolveDefinition 的请求取消、加载状态、错误处理 |
| 运行信息 | 节点只读输入输出、状态、Artifact；不修改执行状态 |
| 展示环境 | locale、翻译、主题令牌、当前只读状态、布局尺寸 |

不暴露 `apiRequest` 任意 URL、内部 Zustand Store、QueryClient 和任意 `setNodes/setEdges` 作为主要插件 API。需要新业务能力时增加有类型的 SDK 方法，不能让示例依赖私有接口。

## 4. 编辑行为必须保持

1. 参数与 InputBinding 保存到 Definition；折叠、搜索文字、临时预览等只进入组件或 Editor Document。
2. 参数在前端变化可以立即预览；服务端的版本、Schema、引用和资源校验仍权威。
3. 复制/粘贴包含精确包引用；目标租户未安装或停用时显示缺失依赖，不替换成最新版或 Code 节点。
4. 断线后的旧引用保留为可修复错误；新增引用严格使用执行边可达前驱，不把资源附件当依赖。
5. 动态端口变化先获取权威契约，预览受影响边与字段，以一次编辑事务应用；失败恢复本次未提交编辑，不回退无关更改。
6. 自动保存的 revision 冲突继续沿用当前处理；晚到的 Provider/契约解析响应不能覆盖较新输入。
7. 不完整草稿可以保存；无法加载声明 UI 时保留节点身份、位置与配置，并阻止依赖该 UI 的编辑。平台标准 JSON 诊断可查看，不能保存猜测后的配置。
8. 已发布/执行记录页面使用只读 SDK Context，同一个业务组件不得在只读模式发送配置更新。

## 5. 内置节点交付矩阵

| 节点/组件 | 本期包与 UI | 本期执行/编译位置 | 验证重点 |
|---|---|---|---|
| Set | `agentx/data`，公开 SDK | TypeScript execute/resolveDefinition | Binding、类型、Lineage、字段输出 |
| List | `agentx/data`，公开 SDK | TypeScript execute/resolveDefinition | filter/sort/take 顺序、items Schema |
| HTTP | `agentx/http`，公开 SDK | TS 组装参数，ctx.http 调现有 Rust Provider 能力 | 凭据注入、出口、二进制、重试和 Trace |
| If | `agentx/core`，统一注册 | Rust 条件/分支逻辑 | case 端口、首中规则、error 分支 |
| Merge | `agentx/core`，统一注册 | Rust readiness/合并逻辑 | 不同合并模式与输入等待 |
| Loop | `agentx/core`，统一注册 | Rust 子图编译与状态机 | 容器布局、并发、恢复、聚合 |
| Approval | `agentx/core`，统一注册 | Rust 挂起、审批与恢复 | 多决策端口、超时、授权 |
| Sub-workflow | `agentx/core`，统一注册 | Rust 版本契约和子执行 | 包闭包、父子 Trace、取消传播 |
| Model | `agentx/core`，统一注册 | Rust Model Provider | 标准 AI 输出、结构化响应、成本 |
| Agent | `agentx/core`，统一注册 | Rust Agent 内核及资源附件 | 六槽、循环、预算、会话与 compaction |
| Code | `agentx/core`，统一注册 | 现有 Sandbox Manager/OpenSandbox | 现有 Runner、网络与输出契约 |
| Start/Exit | `agentx/core` 的边界/节点 UI 注册 | 现有 Workflow 边界语义 | 不生成伪 Attempt、End Schema |

这份矩阵是本期完整交付范围，不以“后续再迁移”代替 UI 注册收口。没有实际收益的 JS 转发包装不增加：核心执行绑定可以由 Worker 直接调用 Rust。

HTTP 插件替换的是节点编排层，不重写 ProviderHttpClient、Vault、出口、调用账本和 Artifact 处理。Set/List 替代实现达到等价门禁后，删除原执行分支与专用重复契约推导；不能长期保持两条执行路径。

内置节点包随平台发布，用户不能删除，但同样产出可检查 Manifest、UI 制品和 conformance 报告。平台发布流水线先构建内置包再生成 Catalog Fixture，Compiler 不在编译 Rust 时另造业务 Manifest。

## 6. 现有组件提取策略

- 先提取 Panel Context、SmartInput 与只读结果能力的稳定接口，再迁移各面板；不顺便重写控件、样式或换表单框架。
- 已经超大或接近 2000 行的文件按现有职责拆分，尤其 Registry、Compiler、Worker 与 Trace；新增单文件也不得超过 2000 行。
- 保留现有测试中的真实业务断言；将直接依赖私有面板映射的测试改为通过注册器加载产物。
- 更新 `studio-catalog.fixture.json` 的生成来源，不手动复制测试 Manifest。
- 统一 `execution-node-panel`、Studio 调试面板和 Trace 页面对 result/traceRenderers 的解析，防止三个页面各自实现插件 Loader。

## 7. UI 门禁

- 上传包的真实 ESM 在新浏览器会话可装载，Hooks 不报错，无第二份 React，无缺失样式。
- 三个复杂样例：字段拖拽映射、远端字段搜索、表格化 Trace 内容；均可在宿主只读模式工作。
- 所有内置节点 UI 通过统一注册路径打开；旧 `ACTION_PANELS` 及按 type 临时兜底逻辑删除。
- 100/300 节点桌面画布与迁移前相同基线比较；不因每个节点动态装载创建重复 Root、全局事件监听和 CSS。
- 多次切换面板/Workflow 后事件监听、挂载 Root 与加载缓存受控；插件错误不影响其他节点的标准查看与图操作。
