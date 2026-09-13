# plan6 自动化验收矩阵

状态：已完成。P6-10 严格复核已为 40 个场景补齐直接断言；完整 `pytest tests/e2e`、32 个 Playwright 用例和正式安装后的插件主链路均通过。遵循 [Kubernetes E2E 规范](../plan/e2e-testing-standard.md)，pytest 是唯一系统级编排入口。

## 1. 测试分层

| 层 | 验证内容 |
|---|---|
| Rust/协议 | 包身份与 Hash、Schema、Catalog、Compiler、引用事务、RPC 结果与状态机 |
| TypeScript/Runner | 真实构建和独立 Node 进程、SDK、分帧/双向请求、trace bridge、清理 |
| UI 组件 | SDK 编辑/只读、变量选择、端口、版本切换、主题和错误 |
| Playwright | 用户实际上传、菜单/按钮、画布拖拽、面板交互、保存、运行和 Trace |
| pytest/Kubernetes | 制品跨域、冷 Worker、Lease/重启/数据库/ClickHouse 故障、部署/最终清理 |

单元/组件通过不等于安装后产品可用；浏览器 mock API 不能代替系统验收。除故障注入和最终断言外，不直接写 DB/OSS 代替被测产品操作。

## 2. Fixture 与组织

- JSON 映射 v1/v2、动态 Provider/Schema/端口、Trace 大内容、无埋点平台 Trace、确定失败/超时/崩溃、后台计时器、孤儿进程和 Worker 冷恢复 Fixture 均已实现。
- Fixture 均由与用户相同的 SDK/build/pack 管线生成；故障插件也从正式模板包派生，不维护生产 Loader 不支持的特殊格式。
- `tests/browser/tests/canvas-plugins.spec.ts` 使用可访问 Role/Label、真实上传和真实鼠标 Handle 拖拽，覆盖管理、画布、运行、Trace 和版本生命周期。
- `tests/e2e/product/test_canvas_plugin_template.py` 在仓库外开发变体并通过页面导入；`tests/e2e/runtime/test_plugins.py` 覆盖镜像握手、进程/内存预算、TTL、崩溃、超时、进程树和 Worker 恢复；ClickHouse 停机/恢复由 `test_playwright.py` 编排。
- 使用现有 product/runtime/observability/publishing marker；不创建平行脚本或额外测试入口。
- 聚焦运行不读取“全量其他套件必须生成”的证据文件；所需 Fixture 自己申请，清理作用域明确。

当前可运行的基础聚焦命令为：

```bash
AGENTX_E2E_ONLY_SUITE=canvas-plugins uv run --frozen --group test pytest tests/e2e/product/test_playwright.py tests/e2e/product/test_canvas_plugin_template.py tests/e2e/runtime/test_plugins.py --values deploy/values/local.yaml --scale-down-development
```

最终验收必须再运行完整入口：

```bash
uv run --frozen --group test pytest tests/e2e --values deploy/values/local.yaml --scale-down-development
```

## 3. 场景矩阵

| ID | 场景与关键断言 | 层/阶段 | 结果 |
|---|---|---|---|
| E2E-01 | 资源菜单显示画布插件，路由/搜索/面包屑正确；查看与管理权限区分 | UI/K8s；P6-01 | 通过 |
| E2E-02 | 上传真实 ZIP，预览元数据，确认导入；Catalog 出现节点，无半安装 | UI/Control；P6-01 | 通过 |
| E2E-03 | 拖入节点、自定义面板修改、变量引用、自动保存、重开、Undo/Redo | UI/Compiler；P6-01 | 通过 |
| E2E-04 | Start→插件→下游引用→Exit；Rust 真实启动 Node，标准 Trace 有输入输出 | 全链路；P6-01 | 通过 |
| E2E-05 | 错误 ZIP/路径/Schema/SDK/入口/native dependency；字段错误清楚，无正式版本 | Control/Runner；P6-02 | 通过 |
| E2E-06 | 相同包版本摘要重复导入幂等，不同摘要冲突；并发确认唯一 | API/DB；P6-02 | 通过 |
| E2E-07 | 导入 v2 和设默认不改 v1 Workflow；不同 Workflow 可运行两个版本 | UI/发布；P6-02 | 通过（P6-10） |
| E2E-08 | 草稿显式整包切版本，显示端口/配置影响；无自动迁移，修复后发布 | UI/Compiler；P6-02/03 | 通过 |
| E2E-09 | 停用后不可新建/Debug/发布；可保存半成品草稿，旧 Deployment 仍执行 | UI/Runtime；P6-02 | 通过 |
| E2E-10 | 停用版本拒绝新 Fork/重新激活；已创建 Attempt 重试/恢复继续原包 | 发布/Runtime；P6-02 | 通过 |
| E2E-11 | 引用中的版本/包无法删除；未引用可删除，事务内并发引用检查有效 | UI/DB；P6-02 | 通过 |
| E2E-12 | 普通用户不能越权导入/下载包/看隐藏 Workflow 引用；可使用已启用节点 | UI/API；P6-02 | 通过 |
| E2E-13 | 生产、Draft Debug、子 Workflow、Fork、评测均携带正确包闭包 | Runtime；P6-02 | 通过（P6-10） |
| E2E-14 | 冷 Worker 仅用 Runtime OSS 装载；无 Control 凭据、无 npm 网络仍成功 | K8s；P6-02 | 通过（P6-10） |
| E2E-15 | 缺失/损坏包明确失败；只重取同摘要；上传取消/TTL/补偿能清理 | API/K8s；P6-02 | 通过（P6-10） |
| E2E-16 | 真实 ESM 无第二份 React、无未解析 import、主题/Portal/样式正确 | Browser；P6-03 | 通过 |
| E2E-17 | 动态 Provider 搜索、分页、取消、权限、晚到响应和资源凭据错误 | UI/Runtime；P6-03 | 通过（P6-10） |
| E2E-18 | resolveDefinition 相同输入稳定；incomplete 可保存、invalid 不推进 revision | Compiler/Runner；P6-03 | 通过 |
| E2E-19 | 动态 Schema/端口被 IR 冻结；运行不重新解析，断线引用与输出类型校验正确 | 全链路；P6-03 | 通过 |
| E2E-20 | 全内置 UI 从统一入口加载；Set/List/HTTP TS 与原语义等价，核心节点完整回归 | 全链路；P6-03 | 通过（P6-10） |
| E2E-21 | RPC 分片/拼包、反向宿主请求、重复响应/通知、日志污染、不支持方法/版本 | Runner；P6-04 | 通过（P6-10） |
| E2E-22 | 多 Item、多端口、零输出、错误 Schema/Lineage/Artifact 的确定性失败 | Runner/Runtime；P6-04 | 通过 |
| E2E-23 | 外部写调用重传不重复；未知结果不盲目重试；结果提交重试不重执行 | Fixture/Runtime；P6-04 | 通过 |
| E2E-24 | 排队/装载/运行超时，协作取消、同步死循环强制回收 | Runner/K8s；P6-04 | 通过（P6-10） |
| E2E-25 | Runner 崩溃、异常后台任务和进程树清理，不影响另一个并发节点 | Runner/K8s；P6-04 | 通过（P6-10） |
| E2E-26 | Worker 重启/Lease 丢失、迟到结果 fencing、新 Worker 冷恢复与正确终态 | K8s/DB；P6-04 | 通过（P6-10） |
| E2E-27 | 并发池、版本隔离、空闲回收、设计时请求不能占满业务容量 | Runner/K8s；P6-04 | 通过 |
| E2E-28 | Node/Runner API 不匹配阻止激活/派发；滚动升级/drain 无孤儿进程 | Helm/K8s；P6-04 | 通过 |
| E2E-29 | Node 原生插件执行与已有 Code/OpenSandbox/Agent 并存，各自资源链路不回退 | K8s；P6-04 | 通过 |
| E2E-30 | 零埋点插件有完整 Node/Attempt Trace；嵌套 Span 在结束前已能查询 | UI/Trace；P6-05 | 通过（P6-10） |
| E2E-31 | 自定义业务类型无需改核心枚举；多份同类型内容保留、专用视图/标准视图可切换 | UI/Trace；P6-05 | 通过 |
| E2E-32 | 并发 Promise 的父子上下文、宿主 HTTP/Model 关联和成本不重复累计 | Trace/账本；P6-05 | 通过（P6-10） |
| E2E-33 | 插件更新/停用后历史 UI 使用旧摘要；缺 renderer/制品时标准数据可诊断 | UI/发布；P6-05 | 通过（P6-10） |
| E2E-34 | Trace 乱序/重复/缺生命周期、Runner/Worker 崩溃和大内容 Artifact | Trace/K8s；P6-05 | 通过 |
| E2E-35 | ClickHouse 停机/延迟、Trace 队列超预算；业务成功仍成功，诊断缺失有标识 | 故障/UI；P6-05 | 通过 |
| E2E-36 | 下载模板到独立目录，AGENTS.md 所有链接可用，check/test/build/pack 成功 | SDK/CLI；P6-06 | 通过 |
| E2E-37 | 模板变体经真实页面上传，复杂交互、Provider、Trace 视图正常 | UI/全链路；P6-06 | 通过（P6-10） |
| E2E-38 | 管理/画布/Trace 关键界面中英、浅深主题、桌面截图与可访问性 | UI；P6-07 | 通过（P6-10） |
| E2E-39 | 100/300 节点性能、反复挂载/卸载、进程池持续运行与缓存清理 | 性能；P6-07 | 通过（P6-10） |
| E2E-40 | 全量旧核心业务回归，成功和故障后的 namespace/进程/对象清理及开发副本恢复 | 全量；P6-07 | 通过（P6-10） |

每个场景保存明确断言，不能仅检查返回 HTTP 200。对于结果未知与诊断不完整，断言具体状态和用户可见提示。

## 4. 内置节点的回归内容

- Set/List：InputBinding 转换、输入数组、稳定排序、过滤、截取、动态输出 Schema、Lineage。
- HTTP：认证、query/header/body、网络出口、二进制 Artifact、超时、错误输出和请求响应 Trace。
- If/Merge：动态端口、首中、else、合并模式、输入齐备与错误分支。
- Loop：空数组、并发、体内失败、取消、Worker 重启、结果顺序、体内审批/子 Workflow。
- Approval/Sub-workflow：多决策按钮、超时恢复、固定版本输入输出、父子 Trace/取消。
- Model/Agent/Code：标准输出、结构化模型结果、六槽和会话预算、已有 Code Runner/网络/Artifact。
- Start/Exit：全部完成/首次返回、End Schema、边界 Span、无虚假节点执行。

测试选择性复用现有 suites；不得为了新包身份把既有业务断言删成只有“节点存在”。

## 5. 性能与资源门禁

在 P6-00 记录同机器、同并发、同输入的基线并冻结数值预算；没有测量不得填写性能提升。至少记录：

| 指标 | 要求 |
|---|---|
| 首次装载与缓存命中耗时 | 分别记录 p50/p95，制品下载不能混进热执行统计 |
| 每节点执行/IPC 开销 | 小计算与 HTTP 两种样例；多 Item 不逐个启动进程 |
| Worker/Node 内存 | 达并发上限后稳定，不随历史包版本无限增长 |
| 进程数 | 不超过预算，取消/退出/空闲回收有可见效果 |
| UI 100/300 节点 | 同样布局与视口，拖拽/面板打开无明显回退，报告帧/长任务 |
| Trace | 事件实时可见，预算耗尽不阻塞结果与取消，完整性标识正确 |

已有明确基线阈值的项目沿用；新增阈值在首次测量后随 P6-00/P6-01 证据冻结，后续不得为了通过测试任意放宽。CI 稳定性与业务性能分开报告。

## 6. 环境、故障与清理

- 每次 run 使用现有 fixture 创建临时 Control/Runtime/Dependencies namespaces、独立入口与证据目录。
- 可以使用 `--scale-down-development`，必须记录测试前副本并在 finally 恢复；不要假定测试前一定为 1。
- 测试默认成功/失败都清理。保留失败现场只能显式 `--keep-on-failure`，完成报告说明剩余资源。
- 数据库/Redis/ClickHouse/Worker 故障仅作用于本次临时环境；不用正式服务承受故障注入。
- 清理上传临时对象、独立开发目录中的测试包、后台开发服务、Node 进程树、port-forward、测试容器；文件删除前确认目标属于测试目录。
- Linux/Kubernetes 结果证明生产执行路径；Windows 本地 Runner/进程树验证单列。CNI/OpenSandbox 强隔离未测时如实说明，不因可信插件假设重写已有安全认证结论。

## 7. 证据与完成判定

证据目录复用 `artifacts/e2e/<run-id>/`：

- 源码提交、平台镜像、Runner/SDK/Node 版本、每个测试插件版本与摘要。
- pytest/JUnit、Playwright HTML/Trace/截图、关键 API 和 DB 状态断言。
- Runtime 包投递与缓存/冷启动日志、进程树、取消/恢复和外部副作用计数。
- Trace 查询中的包版本、父子关系、内容、成本与不完整诊断证据。
- 安装/卸载、临时 Namespace、开发副本恢复与进程清理结果。

测试状态只允许通过、失败或带明确外部前提的未运行；不能无条件 skip 关键插件能力后宣称完成。最终报告逐项映射 E2E ID，至少一个真实模型场景复用当前平台测试能力；真实第三方客户系统未测时说明使用的 HTTP Fixture。
