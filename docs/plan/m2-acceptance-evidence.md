# M2 Workflow 控制面与资源中心验收证据

> 已被 [M2.1 资源中心重构](m2.1-resource-redesign.md) 取代。本页仅保留旧 M2 历史记录，不再作为当前阶段完成证据；新的验收记录见 [M2.1 验收证据](m2.1-acceptance-evidence.md)。

## 自动化检查

- `./scripts/check.ps1` 通过 Rustfmt、Clippy（`-D warnings`）、Rust workspace 测试、OpenAPI 漂移、生成 TypeScript 类型漂移、Oxlint、Vitest、TypeScript、Vite 生产构建和 Kustomize 渲染。
- 后端单元测试覆盖 Workflow Definition 校验、Canonical JSON/Hash、Credential AES-256-GCM 随机 Nonce 和 AAD 认证。
- Platform API MySQL Testcontainers 集成测试覆盖空库重复 Migration、Workflow/Draft/Version/Deployment、Credential、Model、Tool、Skill、LightRAG、Mem0、Grant、审计和跨租户拒绝路径。
- 集成测试使用本地 Fake HTTP Server 验证连接成功、超时和 SSRF 拒绝；Skill ZIP 测试覆盖流式上传、格式拒绝、失败清理、Artifact Hash 和间接依赖循环。
- 前端共 8 个测试文件、16 个测试通过，覆盖路由、主题、国际化、表格、401 合并刷新、multipart、Draft Revision 冲突恢复和 Credential 明文不渲染。
- 变更内最大后端文件 1791 行，最大前端文件 725 行，均低于 2000 行约束。
- `git diff --check` 通过。

## 真实控制面闭环

在 MySQL 8.4 Testcontainer 和真实 Platform API Router 上连续验证：

1. 创建 Workflow 时同事务创建 Draft、Owner Member 和 Workflow Service Identity。
2. 保存 Draft 并对过期 Revision 返回 `DRAFT_REVISION_CONFLICT`，本地未保存内容可由前端冲突对话框保留。
3. 创建 Credential、完整 Model Deployment/Alias、Tool/Version、Skill/Version、LightRAG Resource 和 Mem0 Namespace。
4. Credential 创建与详情响应不包含明文；跨租户 Credential ID 读取被拒绝。
5. Skill ZIP 上传、版本固化、直接及间接依赖展开和循环拒绝可用。
6. 未授权的直接资源和 Skill 递归依赖以结构化 `MissingGrant[]` 返回；补齐 Workflow Service Identity Grant 后校验通过。
7. 从固定 Draft Revision 创建不可变 Workflow Version；相同幂等键重试返回原结果，不同请求 Hash 返回冲突。
8. 发布到 Environment、并发发布唯一 Active、资源撤权阻断新发布、资源停用阻断新发布及回滚旧 Version 全部通过。
9. Workflow 运行入口返回 `503 RUNTIME_UNAVAILABLE`，数据库不创建 Execution、Trace、Approval 或 Evaluation 占位数据。

关键测试入口：

- [Platform API 集成闭环](../../services/platform-api/src/main.rs)
- [Workflow 领域测试](../../crates/agentx-domain/src/workflow.rs)
- [Credential 加密测试](../../crates/agentx-infrastructure/src/credential.rs)
- [Draft 冲突前端测试](../../apps/web/src/features/workflow-designer/workflow-canvas.test.tsx)
- [Credential 脱敏前端测试](../../apps/web/src/features/credentials/credential-detail-page.test.tsx)

## Kubernetes

- `./scripts/k8s-up.ps1` 完成镜像构建、MySQL Ready、Migration Job、应用滚动部署和 Ready 等待。
- `platform-api-migrate` Job 状态为 `Complete`；`_sqlx_migrations` 查询结果为最大版本 `7`、成功 `7`、总数 `7`。
- Platform API 扩容到 2 个副本后为 `2/2 Ready`，连续同源 API 请求均成功。
- Web 通过 Docker Desktop `LoadBalancer` 暴露在 `http://127.0.0.1:8080/`，`/api/v1/bootstrap/status` 同源请求返回 HTTP 200。
- MinIO 缩容到 0 时 Platform API 保持 HTTP 200 且状态为 `degraded`，恢复后回到 `ready`。
- MySQL 缩容到 0 时 Platform API Readiness 返回 HTTP 503，恢复后两个副本均自动回到 `Ready`。
- 当前 Readiness 中 MySQL、Redis、ClickHouse 和 Object Storage 全部为 `ready`。

## 浏览器验收

- 在 1280px 桌面视口打开部署地址，真实登录页无横向溢出，页面无控制台错误。
- 未登录访问 `/workflows` 正确重定向到 `/login`。
- 当前本地数据库已经初始化；验收未猜测、重置或写入未知管理员凭据。受保护页面交互由前端测试和真实 API 闭环覆盖。

验收日期：2026-08-02。
