# V2-07A Scenario Adapter v1

`scripts/v2-07-e2e.ps1` 只负责部署、升级、Migration、恢复顺序和清理；需要业务身份、外部托管服务管理 API 或平台专用网络探针的断言由真实 Scenario Adapter 执行。Adapter 必须接受：

```text
-Action <scenario>
-ContextFile <absolute-json-path>
```

Action 固定为 `Preflight`、`SeedRuntime`、`StartContinuity`、`AssertContinuity`、`AssertRuntimeUpgrade`、`AssertRollbackInvariant`、`StartMigrationContention`、`StopMigrationContention`、`AssertMigrationContention`、`NetworkPolicyMatrix`、`PodSecurityMatrix`、`RotateKeys`、`StopControlDependencies`、`StartControlDependencies`、`AssertRedisRebuild`、`AssertReconciliation`、`AssertNoBusinessResidue` 和 `CleanupFixtures`。

每次调用只向标准输出写符合 `deploy/release/v2-07-scenario-receipt.schema.json` 的 Receipt。`contentSha256` 必须覆盖 Adapter 保存的脱敏原始证据；`assertions` 至少包含一个实际检查。正式 E2E 禁止使用固定 `passed` Fixture、Mock 数据服务或跳过真实请求。Migration Contention Action 必须让一个真实 Migration Job持有目标锁，直到 Stop Action；NetworkPolicy、密钥轮换、Control 离线和 Runtime 连续性均必须访问实际测试环境。

Context 不含 Secret，只包含 Namespace、Profile、Release Manifest、恢复目标和证据目录。Adapter 通过 Workload Identity 或测试 Namespace 中预创建的最小权限 Secret 获取权限。
