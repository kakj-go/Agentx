# plan5 实施计划：Studio交互重建与节点执行闭环

更新日期：2026-09-02。原plan5的M0–M6、F1–F8、V0–V5与plan5.1作为Runtime和节点执行历史基线保留；plan5.2重新打开输入契约、输出构造、Code网络、13类面板、Loop/Merge、视觉和部署门禁。目标与当前状态见 [README.md](README.md)，视觉/交互参照 [demo2.html](demo2.html)。

架构职责与局部收口遵循 [architecture-review.md](architecture-review.md) 的A1–A7，直接映射到本计划任务，不另建一次整体架构重写阶段。

## plan5.2 自然输入与节点交互破坏性重构

- [x] V0：Definition 8.0原地切换为递归`InputBinding`，Condition左右值同源；删除公共`outputProjection`及其Compiler、Runtime、Schema、API、UI和测试路径。
- [x] V0：Code公开参数切换为`runner/inputs/source/outputExample/networkPolicy`；JSON5对象示例推导冻结Schema，旧`outputSchema`和旧网络枚举不接受。
- [x] V1：实现SmartInput、JSON5结构编辑、变量胶囊、Schema转换分级、字段映射行；Inspector默认480/440px，可拖拽420–640px并保存偏好。
- [x] V2：13类节点使用专用面板；IF、Agent预算、Model输出、Set/Code输入、List、HTTP、Sub-workflow、Exit和Context Write完成新交互收口。
- [x] V3：Loop投影迭代开始/结束节点、main/error起止虚拟线、与普通节点一致的外侧端口/快捷新增、空态快捷添加、可撤销大尺寸四角缩放和自动扩容；Merge按模式投影端口，切换模式以可撤销事务清理不兼容连线并展示示例。
- [x] V4：Sandbox只放行Egress Gateway；短期Token携带目标/端口/策略hash；Gateway支持域名、通配域、IP/CIDR、端口段、显式企业私网和永久阻断优先级；三种Runner提供标准代理示例。
- [x] V5：全量Cargo/Clippy、Web、TypeScript、生成物、boundary-check、黄金截图与`pytest tests/e2e`全部通过；清空并按当前DDL重建本地环境，删除临时namespace并恢复开发服务。最终运行ID`8f06dd0c07`，20项在1038.06秒内通过。

plan5.2不增加运算节点。字符串模板只替换变量；算术、解析和结构变换由Code承担。旧草稿、版本、Fixture、OutputProjection、Code outputSchema和网络策略直接丢弃，不增加迁移、双读、转换器或fallback。

## plan5.1 Dify式输入重构

- [x] Definition 8.0原地切换为ValueBinding、ReferenceBinding、TextTemplate与ConditionSpec，删除公开Expression AST、递归default及旧Manifest控件。
- [x] List/Loop使用引用专用输入；IF/List过滤使用类型化条件行；Set、Mapper、Exit、Projection和Context Write使用固定值或Selector。
- [x] HTTP URL/query/header使用TextTemplate，响应只暴露固定顶层字段且body为字符串；JSON解析由Code显式声明输出Schema。
- [x] 前端删除ExpressionBuilder，变量输入一步打开Picker，按完整JSON Schema过滤，并保留可达性与保存时权威校验。
- [x] 13类专用面板消费reference/value/template/structured控件；Set固定值自动JSON推断，001保持字符串。
- [x] demo2黄金截图、Kubernetes HTTP→Code→List→Loop链路及全量本地/Kubernetes门禁通过后关闭plan5.1。

## 0. 硬约束与使用方法

**不考虑历史数据兼容，不考虑旧代码、旧接口或旧协议兼容。**

- 旧数据不迁移；按新契约重建受影响的开发/测试数据。直接更新初始化/基线DDL，不新增数据迁移脚本或逐版本升级migration。
- 不保留旧字段转换、双读、别名、shim、deprecated包装、兼容性fallback、旧接口转发或新旧双入口。
- 切换节点、参数或协议时同时更新调用方、序列化、生成物、Fixture、测试和文档；过时代码直接删除。
- 不为了兼容旧循环保留控制边转换，不为了兼容旧附件保留折叠器，不为了兼容旧审批保留approve/reject包装，不为旧子流程/Code配置保留并行实现。
- 仍有职责的资源能力、权限、Provider、重试、幂等和恢复机制保留；不得把“不兼容”当成改动无关模块、清理无关数据或覆盖用户工作树的理由。
- 历史里程碑的完成标记保留；plan5.2只有上方V5门禁关闭后才能标记完成。

任务状态以证据为准。下列任务是plan5.1及更早版本的历史完成记录；文件定位继续使用路径与符号，不依赖易漂移的旧行号。

| 里程碑 | 最终状态 | 完成证据 |
|---|---|---|
| M0 | 完成 | Definition/Manifest/IR/DTO/DDL/生成物一次性切换，Registry与hash权威及漂移测试通过 |
| M1–M3 | 完成 | 旧数据节点、wait/error_handler/switch、remote_action及旧生成物/入口清除，保留资源执行能力 |
| M4 | 完成 | IF、List、Approval、Exit、HTTP、Sub-workflow、Model和Code均完成配置到真实执行闭环 |
| M5 | 完成 | Loop数组输入、隐式入口、并行、恢复、稳定聚合和三种错误模式通过 |
| F1–F8 | 完成 | 聚焦Studio、13类专用面板、Agent六槽、变量目录、F8调试与节点操作闭环通过 |
| M6 | 完成 | 视觉、业务、性能、边界和Kubernetes 20项完整门禁通过 |

## 1. 实施顺序：先完成最小端到端切片

| 切片 | 内容 | 依赖 / 退出条件 |
|---|---|---|
| V0 | M0规范与契约；复核M1–M3删除边界 | 明确最终规格；新数据只使用一套契约，无兼容层 |
| V1 | F1/F2/F7/F8公共能力；F5接口；F3与F6中的Start→IF→Set→Exit | 创建/接线→配置变量→保存重开→单节点调试→编译→运行可用；第一轮真实视觉对照通过 |
| V2a | M4.2 List、M5 Loop及对应F6面板 | Start数组→List→Loop体内Set→Exit跑通；并行、顺序、错误与恢复有证据 |
| V2b | F4 Agent附件及Agent专用面板 | 六槽选择/授权/保存重开/真实调用同源，不依赖V2a完成以外的产品耦合 |
| V3 | M4.3审批、M4.4完成模式与对应面板 | 自定义第三按钮能恢复；首次返回清理挂起任务；全部完成的顺序/类型正确 |
| V4a | M4.5 HTTP及对应面板 | 查询参数、认证、超时与响应契约在真实请求中生效 |
| V4b | M4.6 Sub-workflow及对应面板 | 显式输入映射实际传入子执行，父端输出类型一致 |
| V4c | M4.7 Model及对应面板 | 结构化Schema进入Provider请求并验证真实结果 |
| V4d | M4.8 Code及对应面板 | 命名输入进入沙箱，结构化结果按声明Schema校验 |
| V5 | F6全量收口与M6 | 全部13类、两主题、桌面布局、业务链路与性能门禁完成 |

F1/F2/F7/F8可先基于已有原生值模型推进；各节点新增配置必须等待其M0契约冻结。前后端按同一垂直切片联调，不等所有复杂功能完成才第一次运行，也不先拆掉仍可运行的基础路径再等待替代品。切片内接通替代实现后原子切换并删除旧路径，不发布新旧双轨，也不以兼容为由保留旧实现。

V2a只关闭基础数组循环切片；M5中的体内审批、提前返回取消等交叉场景随V3的M4.3/M4.4一起验证，不能在这些门禁通过前将整个M5标为完成。V2b是独立资源交互切片，不与Loop实现捆绑验收。V4a–V4d分别关闭，禁止用其中一类通过替代其余三类证据。

## M0 规范、Manifest与契约定稿

状态：完成；M4/M5及前端均消费本阶段冻结的唯一契约。

- [x] 冻结README第2.2节事项：editor聚焦布局与影响范围、品牌主色、六组分类、数组结果字段/Schema、默认值、Code调用协议。未确定事项不靠两套实现并存来回避。
- [x] 核对demo与冻结规则持续一致：Inspector为480/440px并可在420–640px拖拽，Merge为逻辑青、List为转换蓝；原型中的固定业务样例不作为系统默认值。
- [x] 明确13类可创建节点的Catalog边界；保留内部资源执行器，停止把全部 `m5_defaults` 直接当设计器目录。
- [x] 按A1明确内置Registry、数据库Manifest快照、子流程版本契约的所有权和hash一致性；同一类型/版本不能被另一个来源静默覆盖。
- [x] 按A2在现有Rust编译模块中收敛有效输入/输出/动态端口推导，并供校验预览与IR生成复用；前端不另建正式类型推导，Runtime不回查可变Control数据。
- [x] 提取可供Draft保存复用的轻量引用可达性校验：只校验已填写Binding的execution前驱、作用域和端口，不把发布所需的完整配置强加给半成品草稿。保存失败时revision和资源引用均不变。
- [x] Manifest统一分类、图标、显示名、字段/枚举/端口/槽位文案；补新字段的中英文，不再把未知业务名展示为“参数”。前端按同一元数据映射视觉令牌，不新增样式配置服务。
- [x] 冻结List/Loop数组input、Loop outputSelector/并发、审批生命周期/decision、HTTP请求与timeout、子流程inputs、Model结构化输出、Code命名输入/结果的契约。
- [x] 在IR显式冻结Exit定义顺序；`all_complete`只按实际到达Exit聚合，每个字段数组与到达Exit一一对齐，可选缺失写null且Schema同步nullable，必填缺失/null失败。
- [x] 动态端口统一 `case:{id}` / `decision:{id}`；配置、连接、变量目录、Effective Output Contract、Worker输出和回放一致。禁止重复/未知ID，明确保留的固定else/error/timed_out端口。
- [x] 更新domain/IR/Runtime命令事件/Control DTO与初始DDL。新增列或状态直接体现在新库，不写旧数据升级逻辑。
- [x] 使用现有generate-contracts管线重建Definition/Manifest/运行Schema/OpenAPI和前端类型；扩展已有漂移测试与控件白名单对账。
- [x] 为面板契约测试提供来自实际Registry的统一Fixture，不能由前端手工复制一份长期漂移的Manifest。
- [x] 在V0同步直接约束实现的规范，并随对应切片更新docs/02、09、13等当前事实；完成标记仅在实现和门禁通过后写入。
- [x] 触及超2000行的生产文件时按现有职责拆分，如容器编译校验；不借机做无关大重构。
- [x] 跟踪A7现有5项未关闭门禁。已删除wait表同步当前处置/所有权清单；渠道出口失败后直连作为独立高优先级边界整改。不得恢复旧表、放宽网络策略或增加例外白名单来过关。

主要落点：`crates/agentx-domain/src/workflow.rs`、`crates/agentx-node-protocol/src/lib.rs`、`crates/agentx-runtime/src/registry.rs`、`crates/agentx-runtime-contracts/src/{ir,engine,query}.rs`、`services/platform-control/src/catalog_api.rs`、`migrations/{control,runtime}`、`schemas/runtime-v1`、`openapi`。

门禁：当前Schema版本和默认值唯一；旧输入被明确拒绝；生成物漂移测试通过；可创建节点集合和前后端控件契约一致。

## M1 旧独立数据节点删除收口

- [x] 确认builtin_catalog文件及调用已经删除；不恢复旧节点作为兼容实现。
- [x] 清理filter/sort/limit及旧独立数据节点的执行分发、UI角色/颜色/图标、类型别名和参数所有权表残留。
- [x] 对应Fixture改用保留节点；删除只验证旧行为的测试，新增拒绝旧类型的契约检查。
- [x] 区分“移除画布入口”和“删除资源能力”：MCP/Skill/Knowledge/Memory运行器继续服务于Agent，不能误删。

落点：`crates/agentx-runtime/src/registry.rs`、`services/agentx-v2-runtime/src/worker_runtime_builtin.rs`、`services/agentx-v2-runtime/src/worker_runtime_tests.rs`、`services/platform-control/src/work_packages.rs`及相关Fixture、前端node-appearance/types。

门禁：旧类型无法从Catalog创建或通过编译，保留节点及Agent资源能力可用。

## M2 wait、error_handler、switch及旧策略删除收口

- [x] 清理wait专属查询、订阅、token、worker/engine分支及初始化表；approval共享挂起/恢复基础和Trace Wait语义保留。
- [x] 删除error_handler与switch的剩余角色、端口别名、执行器和过期测试；IF按声明的case ID执行。
- [x] 删除settings.onError、End strategy/collectWindowMs的残余读取、旧错误收集状态和兼容默认。
- [x] 以真实error出边推导运行错误路由；错误分支、重试和终态机制不因删除旧配置而丢失。

落点：`crates/agentx-runtime/src/{registry,compiler,state}.rs`、`services/agentx-v2-runtime/src/{engine,suspension}.rs`、`services/platform-control/src/runtime_bff.rs`、`migrations/runtime`与前端旧角色映射。

门禁：审批挂起/恢复仍可用；旧wait/switch/error_handler与旧错误配置无可执行入口。

## M3 remote_action协议删除收口

- [x] 确认Definition/Manifest生成管线已经由runtime-contracts承接，不依赖echo-node旧schemas/openapi子命令。
- [x] 清理NodeActionRequest/Result、远程节点执行、remote节点派生Lifecycle/Poll触发器、路由和生成物残留。
- [x] 保留Node Manifest、仍使用的Provider调用及echo-node Provider Fixture；不误删应用Webhook、Schedule或渠道功能。
- [x] 更新调用方、部署/测试Fixture与协议文档，不保留旧URL转发或适配包装。

落点：`agentx-node-protocol`、`agentx-runtime-contracts`、`agentx-bundle-builder`、Runtime worker/trigger、`services/echo-node`、OpenAPI/Schema。

门禁：可执行路径和生成契约不再暴露旧远程节点协议，Provider/E2E基础能力继续通过验证。历史说明和“拒绝旧输入”的测试可以提到旧名字，不能机械要求所有文档中零关键词。

## M4 节点配置与执行语义

### M4.1 IF：专用ConditionSpec与动态端口闭环

- [x] 将专用条件行的左值/操作符/右值写回 `cases[].conditions[].condition` ConditionSpec；普通字段不再承载递归表达式。
- [x] 固定ID内部生成，显示名可编辑；条件按顺序首个命中，未命中分支关闭，输出key严格等于handle。
- [x] 明确空条件组、AND/OR、比较类型及缺失值行为；增删/重命名分支后连线和引用仍可校验。
- [x] 扩展实例化输出契约与变量目录，不继续将variadic端口只展示成抽象case。

落点：`registry.rs`、`compiler.rs`、`worker_runtime_builtin.rs`、前端condition控件/graph-index/reference-path。

门禁：IF/多个ELIF/ELSE实际分流、分支关闭和下游Merge均正确；生成参数通过真实Rust校验。

### M4.2 List：对用户选中的数组执行

- [x] 增加必需的数组input及类型/基数校验；不能默默使用主端口Items批次替代用户选择。
- [x] 对数组每个元素建立条件求值上下文，再复用filter→sort→takeN算法。
- [x] 明确排序字段路径、升降序、空值策略及限量边界；前端下拉枚举不能退化成任意字符串输入。
- [x] 按M0冻结的数组结果字段与元素Schema输出，供下游Loop和引用目录使用。

落点：`registry.rs`、`compiler.rs`、`engine_parameter_resolution.rs`、`worker_runtime_builtin.rs`与ListPanel。

门禁：从HTTP body内嵌数组或Start数组取值，逐元素过滤排序；空数组、缺失字段、错误类型和takeN边界覆盖。

### M4.3 Approval：统一决策与任务生命周期

- [x] 固定pending/claimed/decided/timed_out/cancelled等生命周期，单独保存业务decision ID；默认approved/rejected仅为默认按钮ID。
- [x] 在任务创建时冻结buttons；校验ID唯一、保留端口冲突和合法字符；显示名不参与路由。
- [x] buttons与decision贯穿Runtime事件/全量快照、Control投影、列表/详情、动作历史及前端类型；不能只新增一个数据库列。
- [x] 统一Decide请求，按版本、候选资格、认领状态、任务快照和幂等规则验证；原子写receipt/decidedBy/reason与resume命令。
- [x] 按A4由Runtime事务做最终任务/执行状态裁决；Control投影只服务于治理展示和前置检查，覆盖投影迟到、重复和重建。
- [x] 删除旧approve/reject路由、枚举分发和SQL状态判断；恢复端口采用decision:{id}，恢复确认不再只识别两种固定结果。
- [x] 超时、取消、执行提前终态与审批任务状态共同收敛，导出投影事件，禁止已失效任务继续恢复执行。
- [x] 前端详情按buttons渲染，筛选/徽标/历史区分任务状态与决策；只改Workflow运行审批，资源授权审批不动。
- [x] 用户选择复用现有组织用户接口；不把“部门主管自动解析”做成没有后端规则的选项。

落点：Runtime `suspension.rs`、`publish.rs::apply_approval_action/approval_action_result`、`engine.rs`、`event_export.rs`；runtime-contracts `engine/query`；Control `projector.rs`、`governance_api.rs`；初始DDL；前端approvals。

门禁：自定义第三按钮从画布保存到详情点击再恢复对应分支；覆盖按钮改名、非法ID、重复提交、权限、超时和任务失效。

### M4.4 Exit：类型、顺序与终态收尾

- [x] 保留first_return/all_complete状态机基础；在IR明确保存Exit定义顺序，不以BTreeMap排序替代。
- [x] first_return决定首个交付后，统一取消剩余激活并让挂起审批失效；清理/事件投影复用现有事务和命令基础。
- [x] 按A5让普通完成、失败、首次返回、显式取消和超时共用Runtime终态收尾规则；同库状态/Outbox原子提交，跨进程取消幂等执行，不新增终态服务。
- [x] all_complete只遍历实际到达的Exit并按定义顺序求值；每个字段为每个到达Exit保留槽位，可选缺失写null，必填缺失/null失败，错误交付按统一错误规则终态。
- [x] 统一工作流发布/API、子流程Manifest、父节点有效输出Schema与变量目录的标量/数组类型。
- [x] 删除旧错误策略、结果别名和旧格式兜底；正式输出只遵循共享End契约。

落点：`agentx-runtime/src/{compiler,state}.rs`、`runtime-contracts/src/ir.rs`、Runtime `engine_persistence.rs::finish_execution`与取消相关路径、`agentx-bundle-builder/src/lib.rs`。

门禁：乱序完成仍按定义顺序返回；数组可被父流程正确引用；先返回的分支不会留下可操作的永久pending审批。

### M4.5 HTTP：请求配置必须影响真实调用

- [x] 增加显式查询参数契约，使用现有URL库编码；保留headers/body的原生动态值。
- [x] 增加认证方式与已有凭据引用，复用Vault、授权和egress；Secret不写入Definition、输出或日志。
- [x] 前端超时写入节点settings，贯穿实际request deadline；消除与硬编码transport超时的不一致，不增加第二份互相冲突的timeout。
- [x] 明确JSON/文件响应支持范围，对宣称支持的输出完成Artifact和Schema校验；不因Manifest有files字段就标为已支持文件下载。

落点：`registry.rs`、Runtime `worker_runtime.rs::execute_declarative_http/declarative_http_request`与HTTP调用辅助、凭据/输出契约、HttpPanel。

门禁：参数编码、真实请求方法/头/体、认证注入与脱敏、超时中断、错误分支及声明的响应类型有端到端证据。

### M4.6 Sub-workflow：接通显式输入映射

- [x] 统一唯一子流程配置与产品入口；选不可变版本后使用该版本输入/输出/Context契约。
- [x] 复用Composite已生成的inputs Schema与快照，但将映射求值、输入校验和create_child真正接通；不能仍只传activation.inputs。
- [x] 删除普通sub_workflow与另一套Composite inputs互不相通的旧配置入口及未映射调用路径。
- [x] 保留父子执行关系、超时取消、结果回传、Context成功提交与递归依赖校验；输出随completion生成正确数组类型。

落点：`registry.rs`、`agentx-bundle-builder/src/lib.rs::node_registry_with_composites`、Runtime `engine.rs`、`composite_execution.rs::create_child/converge_child`、Control版本契约/API及SubworkflowPanel。

门禁：父流程不同字段显式映射给子流程；缺失/错型输入在启动前被拒绝；父端可引用两种完成模式的真实子输出。

### M4.7 Model：结构化输出是执行能力

- [x] 冻结结构化输出开关/Schema的唯一配置；模型摘要从已有资源接口取得安全信息，不复制资源配置或凭据。
- [x] 将配置写入受支持Provider请求，固化有效输出契约；不支持的模型明确报配置错误，不降级伪装成功。
- [x] 校验模型结果的JSON与声明Schema，统一text/structuredOutput/usage等正式字段。
- [x] 清理“仅尝试解析返回文本即视为结构化保证”的判断；普通文本模式与结构化模式的错误行为各自明确。

落点：`registry.rs`、编译有效输出契约、Runtime `worker_runtime_output.rs::openai_chat_request/openai_execution_output`、现有模型Adapter与ModelPanel。

门禁：结构化Schema真实进入请求；合法结果通过、错型/缺字段/非JSON有明确失败行为，变量目录与结果一致。

### M4.8 Code：命名输入与明确的结果契约

- [x] 按M0决定的唯一协议定义inputs mapper、输出Schema、语言与网络策略；Python/JavaScript函数返回和Shell结果方式明确。
- [x] 在现有OpenSandbox路径完成输入求值/传递、调用入口、业务结果提取与Schema校验；继续保留stdout/stderr/exitCode等诊断。
- [x] 删除只为兼容旧Code字符串argv配置保留的分支；不误删Agent工具或MCP进程会话仍使用的进程基础设施。
- [x] 试运行只能生成可审阅的配置建议；不自动替换正式输出Schema，不承诺静态推导任意代码返回类型。

落点：`registry.rs`、运行DTO、Runtime `sandbox.rs::sandbox_command`、`worker_runtime.rs`、`worker_runtime_output.rs::sandbox_execution_output`与CodePanel。

门禁：命名数组/对象输入可消费，结构化结果可被下游引用；输入/输出错型、超时、取消和网络策略有覆盖。

## M5 Loop：前后端共同完成容器语义

- [x] 编译器从parentId与体内DAG推导入口；跨边界连接、嵌套、包含环和体内环明确报错并定位节点。
- [x] 按A3固定Definition、Editor Document和React Flow投影的职责；归属、坐标和相关边在同一编辑事务更新，Serializer不修补运行拓扑。
- [x] 删除显式Loop→body控制边作为入口的编译/序列化路径；编辑期chip和派生线不进入Definition。
- [x] input求值为数组；体内提供`loop.item/loop.items/loop.index`；outputSelector按每轮上下文与迭代结束节点前的体内有效输出求值，错误类型在编译或运行边界报出。
- [x] parallelism限制活跃元素轮次；待执行索引、generation、每轮结果与失败状态可持久化并恢复，不一次性把全部轮次都创建成活跃激活。
- [x] 按输入索引稳定聚合；terminate/continue/remove、空数组、分支关闭、多入口/多末端、体内审批、预算与取消均收敛。
- [x] 新建空Loop即可显示容器并接纳首个子节点；chip有可靠尺寸与可见性，不依赖隐藏的React Flow未测量状态。
- [x] 父相对坐标、边界约束、拖入拖出、resize/自动扩容、保存重开、复制粘贴和撤销重做使用同一套归属规则。
- [x] 自动布局先体内再外层，尺寸由内容撑开；修复已有布局恢复时子节点越界。外部业务节点不可因视觉Group而进入运行容器。

落点：`agentx-runtime/src/{compiler,state}.rs`、`runtime-contracts/src/ir.rs`、Runtime参数求值/检查点；前端 `model/serializer.ts`、`canvas/workflow-flow.tsx`、`canvas/editor-overlays.tsx`、`store/editor-store.ts`、`utils/{connections,layout,studio-clipboard}.ts`。

门禁：从空画布创建Loop并添加首个子节点，保存重开后真实执行数组；并行上限/输出顺序/错误模式/恢复均可证明；无隐藏chip、越界节点或持久化的旧控制边。

## F 前端重构泳道

### F1 统一视觉组件与编辑器框架

- [x] 在现有共享组件中统一Studio紧凑规格、字段行、条件行、KV行、Toggle、加项按钮、满宽模式卡、提示和错误样式；不全站缩小普通企业表单。
- [x] 卡片/Palette/Inspector共用分类色和图标；Start、Exit与普通节点共用一致头部。
- [x] 保留运行页签和轨道；基本属性、高级投影/Context Write次要化，消除重复输出清单和重复说明。
- [x] 按M0确定的方案只调整editor路由工作区；其他企业页面不改。检查1280px及更宽桌面视口的画布空间、滚动和MiniMap占用。
- [x] 为full/compact/minimal缩放层级明确摘要与高度策略；保持连线、焦点、键盘和错误态可辨识。

落点：`apps/web/src/shared/ui`、`styles/globals.css`、`features/workflow-designer/nodes`、`panels/inspectors/panel-shell.tsx`、`panels/node-inspector.tsx`、`workflow-canvas.tsx`、`layouts/enterprise-workbench/enterprise-layout.tsx`。

门禁：V1最小流程的真实截图对照通过；公共控件规格、主题、溢出和键盘行为一致；500/1000节点性能尽早检查。

### F2 节点栏与创建入口

- [x] 264px常驻可收起；仅显示目标节点集，不展示demo中的删除说明分组。
- [x] 修复List误入集成、旧类型视觉映射和无效独立资源节点入口。
- [x] 保留搜索、端口兼容过滤、多输入选择、拖拽、端口快速添加和边上插入；动态case/decision端口也能正确筛选。
- [x] 便签/视觉Group与运行节点职责分开；不存在的控件或能力必须明确报错，不能用通用表单掩盖。

门禁：Catalog与创建器一致；未知/已删类型不能创建；三类创建交互在新布局中正常。

### F3 条件、审批分支与Merge输入行

- [x] IF使用专用变量/操作符/右值条件行；AND/OR按类型组合。显示名可编辑，稳定ID内部维护。
- [x] Approval仅以业务按钮名为主要编辑项；ID不作为普通输入列，改名不改变连线。
- [x] cases/buttons增删同步Handle、连线、摘要和变量目录；固定else/timed_out/error端口不漂移。
- [x] Merge输入行显示业务端口名，三种模式配置按需展开；错误端口常驻。

门禁：增删改分支后保存、重开、引用、编译和真实分流一致；不只断言按钮/testid存在。

### F4 Agent附件管理

- [x] 完成模型、沙箱、工具、技能、知识、记忆六槽真实选择/更换/移除；多槽、多选和必填约束读取Manifest。
- [x] 显示已选资源名、计数、授权/禁用状态与可用操作，复用ResourcePicker及现有授权反馈。
- [x] 节点卡同步附件徽标与摘要；没有附件时提供可发现的配置入口。
- [x] 资源引用直接写入原生resourceReferences，彻底删除旧attachment节点、binding边和折叠重建代码，不做旧草稿转换。

门禁：选择资源→保存重开→编译→Agent真实调用可追踪；撤权/失效/缺必填能阻止非法运行；没有新增一份槽位白名单。

### F5 Start/Exit接口节点化

- [x] Start基础输入名/类型/必填同行，复杂Schema/描述/文件约束再展开；contexts继续在Start，系统变量压成清晰提示。
- [x] Exit同一行对应共享字段定义与本出口取值；紧凑区分成功/错误，完成模式改单列满宽。
- [x] 改名、改型、删除共享输出时同步检查所有Exit；保护初始Exit的现有业务规则不因样式重构丢失。
- [x] WorkflowSettings保留在工具栏；不恢复旧workflow-interface-panel或旧exit-panel包装。

门禁：共享契约与每出口绑定仍是两种不同职责；刷新、撤销重做、多个Exit和两种完成模式均可用。

### F6 13类专用面板收口

面板定制的是业务布局，字段Schema、端口和槽位仍来自后端。不能只是新建13个文件后继续逐字段套通用表单。

| 面板 | 必须完成的专用交互 | 依赖 |
|---|---|---|
| Start | 输入同行编辑、复杂字段展开、contexts | F5 |
| Model | 资源摘要、系统/用户提示、结构化Schema、单份输出 | M4.7 |
| Agent | 真实附件管理行、预算与高级区 | F4 |
| IF | IF/ELIF/ELSE与变量/操作符/右值 | M4.1、F3 |
| Loop | 数组input、outputSelector、满宽错误模式、并行数 | M5 |
| Merge | 三种满宽模式卡与对应Join字段 | F1、F3 |
| Approval | 正确标题/说明、用户选择、业务按钮、超时单位 | M4.3、F3 |
| Code | 命名输入mapper、Monaco、语言/网络、明确结果Schema | M4.8 |
| Set | 字段名+固定值/变量、统一保留字段Toggle | F1、F7 |
| List | 数组input、条件行、紧凑排序下拉、取前N | M4.2 |
| HTTP | 同高method/单层URL、params/headers/body、认证与超时 | M4.5 |
| Sub-workflow | 版本/状态/打开入口、真实inputs mapper、输出类型 | M4.6 |
| Exit | 共享定义/本出口值同行、单列完成模式 | M4.4、F5 |

- [x] 一类面板一个模块，所有生产文件小于等于2000行；复用现有Radix、Lexical和Monaco，不再引入另一套UI库。
- [x] 对应类型完成真实配置闭环后删除NodeInspector通用 `parameterEntries().map` 主路径及旧面板，不保留缺类型时的兼容渲染。
- [x] 中英文键、字段错误、资源状态和可访问名称同步；消除裸露“参数”、`{{key}}`及无说明协议枚举。
- [x] 每面板使用实际生成Manifest完成一轮编辑，并将结果送入真实契约校验；覆盖空态、已配置态、错误态与关键操作。

门禁：13类可见交互与真实参数/契约一致；无占位管理行、无只有提示却不消费的配置、无新旧双面板。

### F7 统一变量与值编辑

- [x] 统一“节点显示名 · 字段”胶囊与明确插入入口；默认展示可用业务字段，避免先出现空白命名空间栏。
- [x] current/first/all/runs、端口基数等按需展开，不把内部Node key当用户显示名。
- [x] 目录从Manifest、节点实例配置、可达关系及目标子流程版本构建；Loop.item/index按容器作用域提供，case/decision端口按实例展开。
- [x] 没有明确目标节点时不展示任意节点输出；普通节点只读取execution边可达前驱，binding/resource边不解锁数据引用。
- [x] Exit成功和错误映射使用各自虚拟目标计算候选；断线后的已有引用保留并显示字段错误，不因候选目录变化自动改值。
- [x] 类型过滤、敏感值限制、缺失值策略和重命名稳定性保留；运行样例不能成为Schema真相。
- [x] 条件/排序/mapper/文本共用字段级能力，保留Lexical光标、IME、选择、删除、撤销和复制粘贴；不引入第二套值协议。

门禁：从选择器插入的变量保存重开后可解析；未连线候选不可选，断线旧引用保存被后端拒绝且revision不变；Loop与子流程数组类型正确；正常文字输入、IME及弹层焦点不受破坏。

### F8 Dify式节点操作闭环

- [x] 节点栏拖拽、端口快捷添加和边上插入共用同一创建结果与自动接线规则；动态case/decision端口也适用。
- [x] 选中节点后在专用Inspector完成配置，卡片摘要、Handle、连线合法性和字段错误即时同步；关闭Inspector不丢失选择或草稿状态。
- [x] 保存并重开后仍定位稳定Node ID、变量引用、面板状态和Editor Document位置；自动保存错误保留dirty状态并定位具体字段。
- [x] 复用现有single_node调试，在Inspector头部运行当前节点；可选择已有执行输入/Pin/Mock作为调试来源，不另造调试协议。
- [x] 在同一Inspector查看Input、Output和Trace，运行失败直接定位节点/字段；修改Pin/Mock后可再次运行，正式Definition与Version不包含调试覆盖数据。
- [x] 分支增删、拖入/拖出Loop、删除节点及相关边均作为可撤销事务；撤销重做后配置、引用、摘要和拓扑一致。

门禁：V1使用可见操作完成“创建并接线→配置变量→保存重开→单节点运行→查看Input/Output/Trace→Pin/Mock复跑→撤销删除”；不以组件存在或按钮可点击代替真实数据流。

## M6 视觉、运行与删除门禁

### M6.1 真实浏览器视觉与交互

- [x] 在隔离测试工作流中覆盖全部13类的空态、已配置态、错误态；当前未实屏的Agent/Merge/Code/Sub-workflow必须补齐。
- [x] 以M0冻结后的demo/规格审阅截图：尺寸、色彩、密度、字段行、弹层、模式卡、滚动、容器与连线均验收。不能只断言组件存在。
- [x] 使用现有1440×900桌面基线并覆盖1280px最小正式桌面布局；浅深主题、中英文、键盘与焦点纳入检查，不增加移动端目标。
- [x] 每类关键编辑后保存并重开，验证Definition与EditorDocument正确；不在用户原草稿中进行会自动保存的探测。
- [x] 对F8完整操作闭环录制截图/Trace证据；节点运行、Pin/Mock和错误定位在新Inspector布局中可发现且不遮挡关键配置。
- [x] 修复并回归已确认的List分类、重复输出、隐藏chip、子节点越界、旧技能控件错误及参数标签问题。

### M6.2 Kubernetes业务端到端

- [x] V1最小链路：使用可见UI创建并运行Start→IF→Set→Merge→Exit；覆盖三类创建入口、变量编辑、分支、保存重开、单节点运行、Input/Output/Trace和Pin/Mock。
- [x] Draft引用门禁：未连线输出不出现在Picker；连线后可选；断线旧引用保留并导致保存返回字段级错误，revision及已持久化草稿保持不变；Exit main/error分别校验。
- [x] 数组链路：HTTP取内嵌数组→List→Loop体内处理→Exit，验证字段选择、`item`/`loop.item`上下文、并行上限、稳定顺序、terminate/continue/remove；在active pendingLoops检查点后滚动重启Workflow Runtime并恢复有序结果。
- [x] 审批链路：自定义第三按钮→任务快照/详情→认领/决策→对应分支恢复；覆盖超时、无权限、重复点击和任务失效。
- [x] 多出口链路：first_return取消包括挂起审批在内的其他工作；all_complete只按实际到达Exit的定义顺序返回等长字段数组，可选缺失为null；父子流程消费的Schema与实际值一致。
- [x] 资源链路：Agent附件、Model结构化输出、Python/JavaScript/Shell命名输入与结构化结果、Bearer/Basic/API Key/custom_json脱敏、HTTP二进制Artifact与超时、子流程显式inputs均有真实执行证据。
- [x] 结合API/数据库/Trace核验结果、命令、任务状态和类型；界面业务数据通过可见操作创建，API/DB用于环境准备、清理和证据断言。

### M6.3 自动化入口与静态门禁

系统级E2E的唯一编排入口是 `pytest tests/e2e`。复用现有Python环境与 `tests/e2e/product/test_playwright.py` 调用TypeScript Playwright；优先复用agentxctl/cargo xtask，不新建PowerShell、bat/cmd或另一套安装/编排方式。

2026-09-01实跑架构检查已通过。原5项分别通过Schema处置清单同步、Provider受控出口收口和生产文件按职责拆分关闭，没有放宽守卫或新增例外。

相关实现与规格：`apps/e2e/tests/m6-workflow-studio.spec.ts`、`m6-local-builtins.spec.ts`、`workflow-multi-exit.spec.ts`、`workflow-canvas-performance.spec.ts`，以及 `tests/e2e/conftest.py` 和现有Runtime集成测试。

| 门禁 | 命令 / 要求 |
|---|---|
| Rust | `cargo test --workspace`及相关集成测试；区分passed/failed/ignored，单独复跑不能冒充整套通过 |
| 前端 | `pnpm --filter @agentx/web test`；`pnpm --filter @agentx/web exec tsc -b --pretty false` |
| 系统E2E | `uv run --group test pytest tests/e2e`，使用已有pyproject.toml/test依赖与临时namespace |
| 性能 | 500/1000节点medianFps≥50/30，p95InputDelay≤50/100ms；通过Python编排调用现有performance spec |
| 契约 | 生成物漂移、UI控件对账、动态端口、引用保存门禁、Exit顺序/可选null对齐、类型与拒绝旧数据测试 |
| 代码边界 | `cargo run -p agentx-boundary-check -- check`；生产文件≤2000行，无旧兼容层/接口包装/双读/双入口；不通过降低守卫要求过关 |
| 清理 | 临时namespace与测试资源最终清理；若按现有规范为减压暂停agentx服务，结束时恢复并记录 |

### M6.4 文档与最终完成定义

- [x] 更新docs/02、03、09、10、11、13、相关Definition说明和总索引，消除旧布局、附件节点、旧错误策略、协议与数据边界说明的矛盾。
- [x] 回顾A1–A7的职责、代码落点与验证证据；架构方向可用、局部实现完成、生产认证通过分别记录，不能互相替代。
- [x] 为每个M/F任务记录代码落点、命令、结果、截图/运行证据和剩余风险；不以“已同步E2E文件”替代E2E成功。
- [x] 更新本计划状态及README，只在视觉、业务运行、类型/安全、性能和清理门禁都满足后宣布plan5完成。
- [x] 核查第0节：没有为历史数据保存转换脚本，没有为旧调用方留下兼容代码。新环境只需要当前契约与当前实现。

## 最终验收证据（2026-09-02）

| 门禁 | 最终结果 |
|---|---|
| Rust格式与静态检查 | `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`通过 |
| Rust全仓测试 | `cargo test --workspace -- --test-threads=1`零失败；2个需要真实钉钉/飞书凭据的外部探针ignored |
| 前端 | `pnpm lint:web`通过（仅既有Fast Refresh提示）；84个测试文件、371项测试通过；`pnpm build:web`通过 |
| TypeScript与Python | Web构建类型检查、E2E `tsc --noEmit`、Ruff、uv lock及16项acceptance通过 |
| 契约与架构 | Definition/Manifest生成物漂移、OpenAPI、`agentx-boundary-check`、diff检查及2000行门禁通过 |
| Kubernetes系统E2E | `pytest tests/e2e --values deploy/values/local.yaml --scale-down-development`：20 passed in 1038.06s；运行ID `8f06dd0c07`；包含Loop active checkpoint后的Workflow Runtime滚动重启恢复，以及Python/JavaScript/Shell通过Egress Gateway访问白名单HTTP与原始TCP、拒绝未列和永久阻断目标 |
| 性能 | 完整产品suite中500/1000节点分别为median 59.9/59.9 FPS、p95输入延迟42.3/37.8ms，通过既定门槛 |
| 清理 | 临时namespace全部删除；开发Control、Runtime和Dependencies Deployment全部恢复为1副本 |

完成结论限定为当前Web桌面产品与本地Kubernetes功能验收。生产容量、PITR/RPO/RTO、gVisor/Kata、供应链证明与V2-08B仍按各自计划验收。
