# M4 验收证据

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；请使用 `agentx-deploy`、`agentx-check` 与 `pytest tests/e2e`。

状态：`done`。最终验收复核日期：2026-08-04。

M4 的阶段 08 和 09 共 24 项原子任务已完成实现，并通过最终快速门禁、Kubernetes E2E、故障恢复、数据库断言和浏览器交互验收。交付范围包括 Definition 2.0、Node Protocol/API、确定性编译和运行状态机、Coordinator/Worker、Checkpoint/Fork、Wait/Approval 与 Execution/Recovery Workbench。完整 Workflow Studio、AI/MCP/Skill/RAG/Memory/OpenSandbox Runner 和 Application/Evaluation/Trigger 全链路接线仍分别属于 M5～M7。

## 1. 已验收能力

- `agentx-runtime` 提供表达式适配、SCC/回边、`n8n_v1` 分支顺序、Readiness、Activation/Attempt/Delivery、Merge、Loop 和固定版本 Sub-workflow 语义。
- `agentx-node-protocol`、[Node 接入文档](../11-node-integration.md)、[Node API](../../openapi/node-api.json)、JSON Schema、协议 Fixture 和 `echo-node` 参考服务已替代旧 `agentx-node-sdk`；没有发布公共语言 SDK。
- MySQL 是 Execution 权威状态；Redis Streams、Transactional Outbox、Lease、Heartbeat、Reaper 和条件提交实现至少一次投递与迟到结果隔离。
- Platform API 已覆盖 Execution、Node Execution、Checkpoint、Wait、Cancel、Fork 和 Side Effect Confirmation；Trigger Gateway 提供独立的 opaque Wait Resume URL。
- Checkpoint State Hash、whole/node/to_node/from_node Fork、输入覆盖、不可逆副作用决策、Timer/Webhook/Form Wait 和 Approval Resume 已接入真实 Coordinator 状态推进。
- Execution/Recovery Workbench 可查看节点 Run、Input、Output、Lineage、Attempt、Logs、Timeline、Checkpoint、Wait、Approval，并预览 Fork 的复用、重跑和副作用决策。

## 2. 快速门禁

执行：

```powershell
.\scripts\check.ps1
git diff --check
```

结果：通过。

- Rust fmt、Clippy `-D warnings`、Workspace Tests 和 Doc Tests 全部通过；其中 Runtime 12 项、Node Protocol 2 项、`echo-node` 6 项、Platform API 23 项、Worker 3 项、Trigger Gateway 3 项、Trace Writer 1 项。
- Migration 0011～0013 覆盖 Runtime、Recovery 与调用期 Broker Handle；最终空库初始化、重复执行和当前 M3 Schema 追加升级结果以本轮门禁为准。
- Platform、Trigger Gateway、Node API OpenAPI/JSON Schema 与生成 TypeScript 类型无漂移。
- Oxlint、12 个 Vitest 文件共 25 项测试、TypeScript 和 Vite 生产构建通过。
- Local/E2E Kustomize、镜像构建入口和 Kubernetes Migration Job 检查通过。
- `git diff --check` 无空白错误；前端源文件最大 1515 行，后端 Rust 文件最大 1945 行，均低于 2000 行限制。

## 3. Kubernetes E2E

完整套件已重复通过；最终复跑复用了本轮已构建并导入集群的镜像：

```powershell
.\scripts\e2e.ps1
.\scripts\e2e.ps1 -KeepNamespace -KeepDevelopmentRunning
.\scripts\e2e.ps1 -SkipBuild
```

保留环境的运行用于浏览器检查，检查结束后已手动清理；最终复跑按默认行为删除 `agentx-e2e` Namespace，并恢复 `agentx` Namespace 的全部原副本数。开发 Namespace 随后通过当前 Migration Job 追加 0013，`platform-api` 恢复 Ready。

| 范围 | 测试 | 结果 |
|---|---|---|
| M2.1/M3 回归 | `m2.1-control-plane.spec.ts`、`m3-control-plane.spec.ts`、`m3-observability.spec.ts` | 通过 |
| M4 Runtime | `m4-runtime.spec.ts` | 1 passed |
| M4 Recovery | `m4-recovery.spec.ts` | 1 passed |
| 故障注入 | Worker/Coordinator 强杀、Redis/ClickHouse 中断 | 通过 |

最新 M4 JUnit 为 2 tests、0 failures、0 skipped，总耗时 59.460 秒。报告和可复现证据入口：

- `apps/e2e/test-results/junit.xml`
- `apps/e2e/playwright-report/index.html`
- `apps/e2e/test-results/artifacts/` 中的 Playwright Trace、网络记录，以及桌面浅色/深色、Fork 弹窗和移动端验收截图
- `apps/e2e/test-results/kubernetes/` 中的服务日志、资源、事件和数据库证据
- [Kubernetes E2E 脚本](../../scripts/e2e.ps1)
- [M4 Runtime E2E](../../apps/e2e/tests/m4-runtime.spec.ts)
- [M4 Recovery E2E](../../apps/e2e/tests/m4-recovery.spec.ts)

## 4. 故障和数据库断言

最终复跑保存的 `m4-database-evidence.txt` 记录：

```text
workerExecution=019fc87c-78a4-7232-b74c-cd76ddfb0d99
coordinatorExecution=019fc87d-34ca-70f1-8cfe-1b89f1b350f2
redisExecution=019fc87d-7649-71c2-a21d-ebe49891d46f
clickHouseExecution=019fc87d-a54e-7fe2-b1da-3d08034c211c
runtimeOutboxPending=0
traceOutboxPending=0
activeTerminalLeases=0
```

自动化套件同时直接断言：

- Worker 强杀后 Execution 最终成功，远程节点只有一个有效成功 Attempt，并且恰有一个 `LEASE_EXPIRED` 失败 Attempt。
- Coordinator 强杀并重建后只依赖 MySQL/IR 恢复，Execution 最终成功。
- Redis 停止期间 Execution Outbox 保持 `pending | failed`，Redis 恢复后最终补投且未交付数为 0。
- ClickHouse 停止不回滚 Execution；恢复后 Trace Outbox 最终为 0，Trace 可查询。
- 终态 Execution 和 Waiting/Approval Execution 均无未释放 Worker Lease。
- Fork 与原 Execution 状态、Checkpoint/Trace 隔离；重复 Resume 和取消后的迟到 Resume 均最多生效一次。

## 5. 浏览器和交互检查

- 在 1440×900 的浅色、深色主题检查 Execution Workbench；10 个 Node Run、9 个 Checkpoint、Merge 的 9 条多来源 Lineage、Attempts 和 Logs 均正确显示。
- Fork 对话框正确预览 7 个重跑节点和不可逆 `Remote Echo` 的 Dry Run/Reuse/Confirm 选项；取消后焦点恢复到 Fork 按钮，并有 Vitest 回归覆盖。
- 在 390×844 检查大纲、数据标签和 Recovery Rail；页面宽度与视口同为 390px，没有页面级横向溢出或内容重叠，局部 Run/Tab 使用可预期的横向滚动。
- Wait 取消态显示 Resume 类型和状态；Approval 恢复态显示 approved/succeeded、Checkpoint 和审批入口。
- `m4-workbench-desktop-light.png`、`m4-workbench-desktop-dark.png`、`m4-workbench-fork-dialog.png` 和 `m4-workbench-mobile-light.png` 已保存到验收 Artifact 目录。
- 浏览器控制台 error/warning 列表为空；终态刷新测试保证节点、Checkpoint、Wait、Trace 和 Approval 不保留运行中旧快照。

## 6. 完成边界和剩余量

- RUN-001～014、REC-001～010、阶段 08、阶段 09 和里程碑 M4 均为 `done`。
- M4 没有接入 AI/MCP/Skill/RAG/Memory/OpenSandbox Runner，没有交付完整 Workflow Studio，也没有提前完成 Application、Evaluation、Schedule/Trigger 的真实运行绑定。
- 项目仍按 M1～M7 七个里程碑推进；M1～M4 已完成。
- M5 的 AGT-001～013 已完成；M6 后续重规划为 STU-001～016 共 16 项，M7 为 INT-001～014 共 14 项。
