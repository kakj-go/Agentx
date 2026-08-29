# Workflow 多结束节点（Exit）重构计划与验收

## 背景与目标

6.0 及之前的画布把"结束"表达为固定虚拟边界 `__end__`：所有分支的 main/error 连线汇聚到同一个点，画布连线嘈杂；输出契约与取值映射全部存在 `WorkflowDefinition.end` 单例字段。

本次重构（Definition 7.0）把终止点改为**多个真实的 `exit` 结束节点**：

1. 画布不再渲染中央 `__end__`；每个 exit 与旧结束节点完全同构（main + error 双入端口），各分支就近连接。
2. 输出契约（字段名/schema/required/sensitive）全局唯一共享，单一真源；在任一 exit 上编辑字段名即全局生效，重命名会同步所有 exit 的映射 key。
3. 取值映射 per-exit：每个 exit 按自己的前驱分别配置成功/错误两组映射。
4. 初始 definition 自动携带一个 `protected` 不可删的 exit；手动添加的 exit 可删除。`__end__` 保留为编译期虚拟锚点，Connections 中不再允许出现。

## 设计决策

| 决策 | 内容 |
|---|---|
| exit 不是执行单元 | 连到 exit 的边折叠进 `terminal_connections`（携带 `targetExit`），出口物化集中在 `materialize_result`；worker 侧没有 outputs namespace，映射需要引用任意前驱输出 |
| 契约与映射分离 | `WorkflowOutput` 拆为 `{schema, required, sensitive}`（留在 `end.outputs`/`end.error.outputs`）；映射 `{字段名 → DynamicValue}` 存 exit 节点 parameters |
| 先到先得 | 多个 exit 并发到达 main 时以 delivery sequence 最小者决定输出；错误按 primary error 到达的 exit 选择映射 |
| 扇出禁令 | 同一 `(source_node, source_port)` 扇出到多个 exit 编译期拒绝（`DUPLICATE_TERMINAL_FANOUT`）；`X.main→exit1` 与 `X.error→exit2` 合法 |
| 必填覆盖 | 每个 exit 必须映射所有 `required` 契约字段（domain `EXIT_REQUIRED_MAPPING_MISSING` + 前端同名校验） |
| 引用范围 | exit 映射引用以扩展图（exit 为虚拟汇点）上的自身前驱为准；错误映射引用须是该 exit 全部错误来源的公共前驱 |
| schemaVersion 7.0 | 照 5.0→6.0 先例硬切换；旧草稿直接失效，无迁移、无 fallback |
| error 策略全局 | fail_fast/collect 与收集窗口仍是全局设置，不 per-exit |
| 对外 API 不变 | `output_schema_json` 依旧从全局契约合成；发布、bundle、应用调用链路零改动 |

## 实施范围

- **Domain**（`crates/agentx-domain/src/workflow.rs`）：`ExitParameters`/`protected`、契约拆分、7.0、`empty()` 模板（初始 protected exit + `start→exit.main`）、exit 校验规则（`EXIT_PARAMETERS_INVALID`/`EXIT_MAPPING_KEY_UNKNOWN`/`EXIT_REQUIRED_MAPPING_MISSING`/`END_BOUNDARY_REMOVED`/`EXIT_NOT_TERMINAL` 语义并入 `INVALID_BOUNDARY_DIRECTION`）。
- **编译器与 IR**（`compiler.rs`、`compiler_normalization.rs`、`runtime-contracts/ir.rs`）：via-exit terminal 折叠、`CompiledExitV1`、`start_to_exit`（替代 `start_to_end`）、per-exit 引用/类型/coerce 校验、`DUPLICATE_TERMINAL_FANOUT`、可达性适配。
- **运行时**（`state.rs`、`engine_persistence.rs`、`engine_persist_machine.rs`）：`EndDelivery.targetExit`、物化按到达 exit 选映射、部分执行子图适配、`execution_end_deliveries.target_exit_id` 列。
- **控制面**（`workflow_api.rs`）：draft 三处 `schema_version` 硬编码升 7.0。
- **前端**（`apps/web`）：`ExitNodeData` 类型、`addExit`/删除保护（`removeSelected` + ReactFlow remove change 拦截）、serializer 7.0 双向、画布只合成 `__start__`、ExitNode 组件、连线校验（exit 双入端口 variadic）、ExitPanel（全局契约组 + 本出口映射组 + 契约改名联动）、palette 内置条目、布局尺寸、i18n（zh/en 同构）。
- **E2E**：新增 `apps/e2e/tests/workflow-multi-exit.spec.ts`（初始 protected exit 不可删、第二个 exit 可删、共享契约 + 各自映射、保存断言、运行断言输出取自实际到达 exit 的映射）；`m6-workflow-studio`、`m2.1-control-plane`、`m6-local-builtins`、`resource-grant-requests` 的旧 `workflow-end` 交互全部迁移到 exit 节点；pytest suites 新增 `workflow-multi-exit`。

## 验收状态

| 门禁 | 状态 |
|---|---|
| `cargo test --workspace --lib`（domain/runtime/bundle-builder/v2-runtime 等） | 通过（含 compiler_tests 全量 fixture 迁移与新增 via-exit/扇出/前驱/空工作流用例） |
| `cargo test --workspace --no-run`（含集成测试编译） | 通过 |
| 前端 vitest（66 文件） | 通过（含 exit-node 组件、store protected 拦截、ExitPanel 契约联动、definition-validation exit 校验） |
| `tsc -b && vite build` | 通过 |
| Kubernetes E2E（`pytest tests/e2e`，临时 namespace） | 待集群执行（`workflow-multi-exit` suite + 迁移后的 product-closure/control-ui suites） |

## 遗留与后续

- exit 不是执行单元，trace 中没有 exit 的 node_execution；出口命中体现在 end deliveries 与终态输出。若调试需要更细粒度的出口 trace，后续可在 finish_execution 记录 exit 命中事件。
- `tests/runtime_slice` 集成用例（需 MySQL fixture）建议在集群验收时一并补双分支多 exit 物化断言。
