# P3-04 外挂能力配置期授权 ADR

状态：accepted（2026-08-25）

## 问题

MCP Server Version 可以引用 Credential 和 stdio Runtime Sandbox。MCP Server 是全局资源，创建或编辑它时不存在 Workflow，也不存在 Workflow Service Identity，因此不能复用
`/workflows/{id}/resource-options`，更不能用任意 Workflow 的运行身份替 MCP Server 获取配置依赖。

普通 Credential/Sandbox 列表只证明资源存在，不能证明当前 MCP Owner Department 有权把它固化到 Server Version。把普通列表包装成 `ResourcePicker` 的 `authorized` 选项会绕过现有资源授权模型。

## 决策

配置期和运行期使用两类明确分离的授权主体：

1. MCP Server 配置期依赖以 Server 的 `owner_department_id` 作为 Department Grant Subject。
2. 同一 Department 管理范围内自有资源视为配置期已授权；跨 Department 资源必须具有精确的 Department Resource Grant，或通过现有多部门会签流程申请。
3. Workflow 发布和运行仍只接受该 Workflow Service Identity 的完整传递 Grant。Department Grant 不进入 Agent Bundle，不替代 Runtime Grant，也不能被 Worker 当作执行凭据。
4. MCP Server 不新增 Runtime Identity。实际外部 Effect 始终由调用它的 Workflow Service Identity 授权、记账和撤权。
5. Grant Request 使用统一 Subject：`department | workflow_service_identity`。请求项、会签、通知和审批保持共用；只有 Workflow Subject 在批准或撤权后推进 Runtime Admission/Policy Epoch。

## API

配置期资源选择使用：

```text
GET  /api/v1/departments/{department_id}/resource-options
POST /api/v1/departments/{department_id}/resource-authorizations
POST /api/v1/departments/{department_id}/resource-grant-requests
```

返回状态沿用统一 Resource Picker：

```text
authorized | grantable | requestable | pending | rejected | unavailable
```

MCP 创建或更新在写入新 Server Version 前重新校验选中的 Credential、Environment Credential 和 Runtime Sandbox 精确版本，防止跳过 Studio 直接调用 API。

## 影响

- MCP Studio 可以复用统一 Picker、授权、申请和审批 UI，但必须先选择 Owner Department。
- Runtime Bundle/Grant、Agent Core、Tool Router 和两类 Sandbox 的隔离契约不变。
- 不创建兼容解析、历史 migration、隐式全租户访问或配置期到运行期的 Grant 继承。
