# V2 契约版本窗口

| 契约 | 当前生产版本 | Consumer 支持窗口 | 初始阶段行为 |
|---|---:|---|---|
| Execution Spec Bundle | 1 | 当前与上一版本 | 仅接受 1 |
| Compiled IR | 1 | 当前与上一版本 | 仅接受 1 |
| Integration Event/Command | 1 | 当前与上一版本 | 仅接受 1 |
| Runtime Internal API | 1 | 当前与上一版本 | 路径固定 `/internal/runtime/v1` |
| Worker Protocol | 1 | 当前与上一版本 | 仅接受 1 |

生产者只输出当前版本。V2-00 的 Command、Event、Execution Result、IR、Bundle/Work Package、Worker Protocol、Internal API 请求、Receipt 和 Query/Publish 响应均使用数字版本 `1`，Serde 边界拒绝未知字段和版本 `2`。Consumer 在未来版本 2 发布时可接受 1 和 2；版本 3 发布前必须完成版本 1 的 Bundle/队列/滚动实例清理。超窗 Bundle 在 Prepare 和 Rollback 阶段拒绝，不能等到执行时失败。

升级顺序固定为：Expand Schema → 部署兼容 Consumer → 部署新 Producer → 等待旧对象/消息和旧实例排空 → Contract Schema。回滚应用不得回滚 Schema，不得降低 Admission Epoch，也不得重新激活已撤销 API Key、Grant 或 Tenant。

混合代测试矩阵：旧 Consumer+旧 Producer、兼容新 Consumer+旧 Producer、兼容新 Consumer+新 Producer；超窗版本、未知字段和新枚举值必须在边界处明确失败。V2-00 只存在版本 1，因此代码测试当前验证 `1` 成功、`2` 失败。

## V2-04 破坏性 v1 重写例外

V2-04 在项目尚未进入生产且明确不保留开发数据的前提下，原地收紧数字版本 `1`：Bundle、Work Package、Worker Result 和 Internal API 的必需字段以 V2-04 Schema 为准。缺少 `RuntimePolicyV1`、授权快照、Worker 兼容矩阵、资源闭包或 `modelEvaluatorExecutions` 的 V2-03 对象即使标记版本 `1` 也必须拒绝，不增加字段别名、默认补值、双读或 Feature Flag。

该例外意味着 V2-04 不支持旧/新 Runtime 实例混合滚动。Migration Runner 在检测到已应用 `0003` 且存在 Bundle、Session、Invocation 或 Execution 业务数据时拒绝应用 `0004`；开发部署必须显式使用 `-RecreateV2Data` 清空两个 V2 MySQL、Runtime Redis 和三域 Bucket，再从 Bootstrap 和正式 API 重建。回退只能回退应用镜像，不能把已重建的数据域交给 V2-03 二进制。

## V2-05 破坏性 Event/Projection/Trace v1 重写例外

V2-05 同样在无生产数据的开发阶段原地重写数字版本 `1` 的 Integration Event、Governance Snapshot、Projection 和 Trace Envelope。缺少全局 Cursor、对象 Version、Content Hash 或类型化 Payload 的 V2-04 Fixture 必须拒绝；不提供字段别名、转换、双读、双写或 Feature Flag。

Migration Runner 在已有 V2-04 业务数据时拒绝应用 `0005`/Observability `0002`。切换必须显式使用 `-RecreateV2Data` 重建 Control/Runtime MySQL、Runtime Redis、ClickHouse 和三个 Bucket，旧 Redis Trace 消息和旧 Projection 不可复用；应用回退不能把重建后的数据域交给 V2-04 二进制。最终破坏性 E2E 见 [V2-05 证据](../evidence/v2-05.md)。

“当前与上一数字版本”的常规演进窗口从 V2-05 重写后的 v1 开始；下一次发布数字版本 2 时必须按 Expand → 兼容 Consumer → 新 Producer → 排空 → Contract 顺序同时验证版本 1 和 2。
