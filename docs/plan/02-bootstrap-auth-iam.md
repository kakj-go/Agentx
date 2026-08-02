# 阶段 02：初始化、认证和 IAM

## 1. 目标与用户价值

让新部署可以安全创建首个企业和管理员，并为后续所有页面、API、资源和 Workflow 提供真实身份、租户隔离、操作权限与数据范围。

## 2. 当前状态和进入条件

- 状态：`done`。
- 进入条件：[阶段 01](01-contracts-and-foundation.md) 的 Migration、API Error、Tenant Context 基础和前端 Client 完成。
- Setup、Login、首次改密、组织和角色页面已接入真实 API，其他后续阶段业务页面仍保留 Mock 展示。

## 3. 范围和不做内容

实现一次性 Bootstrap、本地账号 JWT、单公司内的部门、用户、角色、权限和 Workflow Service Identity 基础。不实现 Tenant CRUD、租户切换、OIDC 登录、LDAP、SCIM 或通用身份治理。

## 4. 领域对象、状态和不变量

- Bootstrap 状态只有 `required` 和 `completed`，完成后不可通过公开 API 回退。
- 一个部署只有一个 Tenant；tenant_id 保留但客户端不能创建或切换 Tenant。
- User 状态为 `invited`、`active`、`disabled`；被禁用用户的 Refresh Session 全部撤销。
- Role 属于单一 Tenant；系统预置角色可复制但不能删除其定义来源。
- Department 使用父子结构并维护闭包关系，移动部门必须在事务内重建受影响路径。
- Permission 分为操作权限和数据范围；拒绝优先于隐式允许，未授权默认拒绝。
- Workflow Service Identity 属于 Tenant 和 Workflow，不允许交互式登录。
- Company Admin 管理全公司；Department Admin 管理其作用部门及后代。新用户由服务端设置固定的一次性初始密码 `123456`，首次登录必须修改；正式密码仍须满足 12–128 位规则。

## 5. 数据和 Migration

主要表：

- tenants、tenant_settings、bootstrap_state
- departments、department_closure
- users、user_credentials、user_departments
- roles、permissions、user_roles、role_permissions
- refresh_sessions；Workflow Service Identity 本阶段只冻结类型边界，持久化按阶段 03 实施
- audit_events

密码只保存 Argon2id Hash。Refresh Session 保存 Token Family、JTI Hash、过期时间、轮换和撤销状态，不保存可直接使用的 Refresh Token。

## 6. REST API、Port 和事件

- `GET /api/v1/bootstrap/status`
- `POST /api/v1/bootstrap`
- `POST /api/v1/auth/login`
- `POST /api/v1/auth/refresh`
- `POST /api/v1/auth/logout`
- `GET /api/v1/auth/me`
- `/api/v1/departments`、`/users`、`/roles` 和 `/permissions`

Access Token 默认短时有效，前端只保存在内存并使用 Bearer Header；Refresh Token 通过 Secure、HttpOnly、SameSite Cookie 轮换。OIDC 只定义 `IdentityProvider` Port，未来成功认证后仍转换成相同 `ActorContext`。

业务事件：`TenantBootstrapped`、`UserInvited`、`UserDisabled`、`RoleChanged`、`PermissionChanged`。事件和写操作同时产生审计记录。

## 7. 后端和前端改动

- Platform API 新增 bootstrap、auth、departments、users、roles 模块。
- Application 层增加密码验证、Token 签发、权限判断和数据范围 Port。
- Infrastructure 层实现 JWT、Refresh Session、Argon2id 和 IAM Repository。
- 前端新增 `/setup`、`/login`、认证恢复、受保护路由和 401 刷新队列。
- `/organization` 和 `/roles` 接入真实 API，并新增用户邀请、禁用、角色分配和部门移动界面。
- 权限不足统一展示 403 页面或禁用操作，不依赖隐藏按钮作为安全措施。

## 8. 实施任务

| 编号 | 状态 | 依赖 | 交付物 | 验收条件 |
|---|---|---|---|---|
| IAM-001 | done | FND-003、FND-007 | Bootstrap、Tenant、User 和 Credential Schema | 并发 Bootstrap 只有一个请求成功 |
| IAM-002 | done | IAM-001 | Argon2id 密码、JWT Access Token 和 Refresh Session | 错误密码不泄露账号是否存在，Token 可验证过期和签发者 |
| IAM-003 | done | IAM-002 | Refresh 轮换、重放检测、注销和全端撤销 | 已轮换 Token 再次使用会撤销同一 Family |
| IAM-004 | done | IAM-001 | Department 邻接表、Closure Table 和移动用例 | 禁止循环父子关系，移动后子部门范围正确 |
| IAM-005 | done | IAM-001、IAM-004 | Role、Permission、User Role 和数据范围引擎 | 权限矩阵覆盖本人、部门、子部门和全公司 |
| IAM-006 | done | IAM-005 | Workflow Service Identity 类型边界 | Identity 无登录能力且始终绑定 Tenant 与 Workflow |
| IAM-007 | done | IAM-002、IAM-005 | Tenant/Auth/Permission 中间件 | 客户端伪造 tenant_id 无法越权，禁用用户立即失效 |
| IAM-008 | done | IAM-001–007 | IAM 审计事件 | Bootstrap、部门、用户、角色和权限写操作可追溯 |
| IAM-009 | done | IAM-007、FND-011 | Setup、Login、Change Password、会话恢复和受保护路由 | 刷新页面可恢复登录，刷新失败安全退出 |
| IAM-010 | done | IAM-004–005、IAM-009 | 部门、用户和角色真实页面 | 页面支持创建、编辑、移动、禁用、过滤、冲突和权限不足 |
| IAM-011 | done | IAM-007 | `IdentityProvider` OIDC Adapter Trait | 接口能映射外部 Subject，但无首期 OIDC 实现和入口 |

## 9. 失败、安全和幂等边界

- Bootstrap 使用数据库唯一约束和事务，不能只检查进程内标记。
- 登录进行速率限制，密码和 Token 不进入日志、Trace 或审计详情。
- 登录限速状态保存在 MySQL，以规范化用户名的 SHA-256 作为键；默认 15 分钟窗口内失败 5 次锁定 15 分钟，所有 Platform API 副本共享结果。
- Access Token 过期不等于 Refresh Session 失效；Refresh 重放触发 Token Family 撤销。
- 删除存在用户或子部门的部门必须先迁移归属，不允许产生孤立关系。
- 角色更新使用乐观锁，防止管理员覆盖其他人的权限修改。
- 所有 Repository 查询以 tenant_id 开头；按 ID 查询后仍验证数据范围。

## 10. 测试

- 密码、JWT、轮换、过期、重放和注销单元测试。
- Bootstrap 并发、部门移动、角色冲突和租户隔离集成测试。
- Permission Matrix 属性测试，覆盖部门树和指定资源组合。
- 前端登录恢复、401 刷新合并、403、用户过滤和角色编辑测试。
- 两租户端到端越权测试，覆盖 URL ID 猜测和搜索接口。

## 11. 验收门禁

- 未初始化部署只能访问 Bootstrap 和健康接口。
- Bootstrap 完成后不能再次创建首个管理员。
- JWT 刷新、撤销、用户禁用和密码修改行为正确。
- 跨租户访问始终失败，部门数据范围符合权限矩阵。
- Organization 和 Roles 页面不再依赖 Mock 数据。

## 12. 对后续阶段的稳定输出

- `TenantContext`、`ActorContext` 和认证中间件。
- 操作权限与数据范围引擎。
- Workflow Service Identity 基础。
- IAM 审计记录和受保护前端路由。
