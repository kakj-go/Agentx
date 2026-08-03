# Kubernetes 界面 E2E 测试规范

## 1. 目的和适用范围

Playwright E2E 是所有已完成业务功能的阶段门禁，用于证明用户可以在真实浏览器中通过可见界面完成业务闭环。快速单元、契约和组件测试继续由 scripts/check.ps1 承担，不能用它们替代界面 E2E。

本规范适用于 M2.1 及之后所有新增或修改的页面、表单、菜单和业务按钮。

## 2. 环境模型

- 每次验收创建独立 agentx-e2e Namespace 和全新 PVC。
- 部署 MySQL、Redis、ClickHouse、MinIO、Migration Job、Platform API、Web 和当前阶段需要的业务服务。
- M2.1 默认部署 Echo MCP，不依赖公网服务。
- 通过 kubectl port-forward 暴露 Web，同源 /api 代理保持与生产一致。
- 默认在成功或失败后删除 Namespace；KeepNamespace 只用于人工排障。
- E2E 不复用 agentx 开发 Namespace 的数据或登录状态。

## 3. 数据和操作规则

- Bootstrap、登录、资源创建、编辑、授权、发布等业务数据必须通过页面按钮、表单、拖拽和菜单创建。
- API 或数据库只允许用于环境准备、清理、故障注入和最终证据校验，不能替代被测 UI 操作。
- 测试优先使用可访问角色、Label 和稳定业务名称定位，不依赖 CSS 层级、内部组件类名或随机 ID。
- 画布连线必须通过真实鼠标拖动 Handle，不允许直接注入 React Flow State。
- 文件场景使用浏览器文件选择或拖拽输入，不直接写对象存储。

## 4. 按钮覆盖规则

- 每个新增或修改的业务按钮至少有一个 Playwright 真实点击路径。
- 新建、编辑、保存、发布、授权、测试连接和调试等命令必须断言可见结果或服务端错误。
- 资源授权必须从统一 `/resource-grants` 页面完成；资源详情页不得保留第二套授权入口。
- 删除、撤权、覆盖、调试高副作用 Tool 等危险操作同时覆盖取消和确认。
- 乐观锁操作至少覆盖一次 409，且确认页面不会静默覆盖服务器状态。
- 权限受限操作同时覆盖按钮隐藏和后端拒绝或 /403 路由。
- 未接入 Runtime 的按钮保持禁用，并断言不会创建伪 Execution。

## 5. 通用界面门禁

- Chromium 桌面视口固定为 1440×900，后续阶段按风险补充其他桌面尺寸。
- 至少覆盖 zh-CN、en-US、浅色和深色状态。
- 页面不得出现原始翻译键、Invalid Date、Secret 明文或不可恢复的错误。
- MCP Schema 必须断言字段、类型和必填信息，不能只断言原始 JSON；Skill Markdown 必须通过富文本工具栏和 `contenteditable` 完成编辑。
- 多字段 Dialog 在 1440×900 下必须断言主要提交按钮处于视口内；仅真实超过浏览器可用高度时允许滚动。
- 所有关键页面必须覆盖加载完成、空结果、失败提示和权限不足中的适用状态。
- 表单使用可访问 Label，Dialog 具有稳定名称，图标按钮具有 aria-label。

## 6. 证据和保留

Playwright 生成：

- HTML Report
- JUnit XML
- 成功与失败 Trace
- 失败 Screenshot 和 Video

编排脚本额外保存 Kubernetes 资源、事件、Platform API 日志、Echo MCP 日志和 port-forward 日志。验收证据文档记录命令、结果、报告路径和关键业务闭环，不提交包含 Secret 或真实业务数据的附件。

## 7. 运行命令

完整阶段验收：

    ./scripts/e2e.ps1

复用已构建镜像：

    ./scripts/e2e.ps1 -SkipBuild

保留失败现场：

    ./scripts/e2e.ps1 -SkipBuild -KeepNamespace

有界面调试：

    ./scripts/e2e.ps1 -SkipBuild -KeepNamespace -Headed

## 8. 完成定义

功能任务只有在单元、集成、OpenAPI、前端快速检查和临时 Kubernetes E2E 全部通过后才能标记 done。E2E 失败时对应阶段保持 in_progress 或 blocked，不允许用人工点击结果替代自动化证据。
