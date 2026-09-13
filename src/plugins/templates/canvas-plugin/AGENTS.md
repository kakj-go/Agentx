# Agentx 画布插件开发约定

先阅读 `docs/node-contract.md`，再阅读 UI、Runtime、Trace 和测试说明。修改前运行 `pnpm check && pnpm test`；完成后运行 `pnpm build && pnpm pack:plugin`。

节点 ID、端口、参数 Schema 和输出 Schema 是持久协议。修改它们时提升 `typeVersion`，修改包时提升 `packageVersion`。插件运行代码只能导出 `execute(context)`；不要自建 RPC 服务、写 stdout 协议、启动常驻后台任务或保存跨调用进程内状态。

配置面板通过 `updateParameters` 修改业务参数；必须处理 `readOnly` 和 `fieldErrors`。使用宿主 React/组件、`locale/theme/portalRoot/assets`，不导入 Agentx 仓库内部文件。动态选项通过 `host.design.invokeProvider` 并传递 AbortSignal；Provider 的第二个参数是 Runtime 提供的 `ProviderContext`，只能访问节点已选择的资源快照。动态端口和 Schema 由 `resolveDefinition` 返回。执行时参数已经完成 Binding 解析；返回声明端口上的 Item，并保留或用 SDK helper 创建 Lineage。

平台自动记录 Node/Attempt 输入输出。业务过程使用 `context.trace.span`，业务内容必须有命名空间类型和正整数版本。HTTP、模型、凭据和 Artifact 应使用 SDK 宿主能力；遵守 deadline、AbortSignal、幂等键和结果未知语义。

交付说明需列出契约变化、执行的命令、结果及尚未验证的真实外部服务。模板测试通过不能替代 Agentx Kubernetes E2E。
