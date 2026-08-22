# M1 基础管理闭环验收证据

> 历史命令说明：本文中的 PowerShell、Shell、deployment Profile 和核心 Kustomize 命令只记录当时验收事实，相关入口已经删除，不得作为当前操作入口；请使用 `agentx-deploy`、`agentx-check` 与 `pytest tests/e2e`。

## 自动化检查

- `scripts/check.ps1`：Rustfmt、Clippy、Rust 单元/集成测试、OpenAPI 漂移、TypeScript 契约漂移、Oxlint、Vitest、TypeScript、Vite 和 Kustomize 检查。
- `services/platform-api/src/main.rs`：Testcontainers MySQL 空库和重复 Migration、并发 Bootstrap、登录限速和审计、Refresh 重放撤销、受邀用户首次改密、部门移动 Closure、权限 API、跨租户查询隔离、Artifact Hash、Outbox 事务回滚、租约、失败重试和幂等测试。
- `services/platform-api/src/security.rs`：Argon2id Salt/Verify 与 JWT Kind、Issuer、Audience 测试。
- `apps/web/src/shared/api/client.test.ts`：并发 401 合并为单次 Refresh。
- `crates/agentx-service-kit/src/lib.rs`：必要依赖失败、可选依赖降级和全部依赖恢复后的 Readiness 状态转换。
- `apps/web/src/app/auth-guards.test.tsx`：Setup、Login、首次改密、受保护路由和统一 403 权限守卫。
- API 错误响应测试校验 JSON `requestId` 与 `x-request-id` 响应头一致。

## 真实 API 主链路

在临时 MySQL 8.4 容器和真实 Platform API 上验证：

1. 空库返回需要 Bootstrap。
2. 初始化公司、根部门、Company Admin、内置角色和权限。
3. Company Admin 创建部门和 Department Admin。
4. Department Admin 以临时密码登录，只获得 Change Password Token。
5. 修改正式密码后创建下级部门和成员。
6. Department Admin 只能查询作用部门与后代；向根部门写入返回 403。
7. Refresh Token 轮换成功；重放旧 Token 后整个 Token Family 被撤销。
8. 新用户以 `invited` 状态创建，首次修改正式密码后转换为 `active`。
9. `/api/v1/permissions` 只返回操作者持有、可安全用于角色配置的权限。

## Kubernetes

- `agentx/platform-api:dev` 与 `agentx/web:dev` 镜像构建成功。
- `scripts/k8s-up.ps1` 完成 MySQL Ready、`platform-api-migrate` Job、全部 Deployment 滚动启动和 Ready 等待。
- `platform-api-migrate` Pod 状态为 `Completed`，Platform API 与 Web Pod 状态为 `Running/Ready`。
- local Overlay 使用 Docker Desktop `LoadBalancer` 在主机端口 `8080` 暴露 Web；通过 `http://127.0.0.1:8080/` 访问首页，并通过同一 Origin 访问 `/api/v1/bootstrap/status`。

验收日期：2026-08-02。
