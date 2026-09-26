# P7-E：知识库管理面

## 1. 目标与边界

把知识库从"两个连接选项"升级为有文档管理的资源：**上传 → 外部索引 → 索引状态 → hit-testing 检索调试**最小闭环。检索与索引继续由外接服务（LightRAG/RAGFlow）承担，平台不建本地向量库（对标结论：Dify 的 RAG 管理深度是其护城河，但 37 向量库长尾不值得追；Agentx 以"管理面 + 受控调试"对齐最小可用形态）。

范围与首期能力矩阵：

| 能力 | LightRAG | RAGFlow |
|---|---|---|
| 文档上传/索引 | 支持（`documents/text` 已有协议） | 不支持写路径（保持 `RAG_OPERATION_UNSUPPORTED`） |
| 文档状态跟踪 | 支持（query 回查/索引探针） | 不适用（外部自管） |
| hit-testing 检索调试 | 支持 | 支持（retrieval 已有协议） |
| 分段预览 | hit-testing 命中 chunk 展示 | 同左 |

不做：PDF/Word 等二进制文档抽取（仓库内零抽取能力，首期限定 `text/*`、`.md`、`.json`、`.csv`）；本地 embedding/向量库；RAGFlow 写路径；re-rank 配置界面；知识库版本化发布。

## 2. 现状事实

- 资源模型已具备：`rag_connections`（provider enum lightrag/ragflow、endpoint、health_path、credential_id、configuration_json）与 `rag_resources`（external_resource_id、**sync_status 字段已存在**：unknown/syncing/synced/failed）——`deploy/migrations/control/0001_initial.sql:929-962`；
- API：`/api/v1/knowledge/connections`（含 test-connection 落 `resource_health_checks`）与 `/api/v1/knowledge/resources`（`external_resource_api.rs:22-58`，权限 knowledge:view/manage）；六态授权覆盖 rag（`resource_api.rs:86`）；快照与 Runtime binding 已通（`workflow_resources.rs:603-639`、`runtime_resource_binding.rs:181-191`）；
- Runtime RAG 协议私有于 runtime crate：`rag_query_request`/`finalize_rag_response`（`worker_runtime_output.rs:487-616`，LightRAG query + `documents/text` insert、RAGFlow `api/v1/retrieval`）；Agent 附件槽复用同一套（`worker_runtime_agent_core.rs:1596-1636`）；
- 前端仅两个小文件：`knowledge-page.tsx`（33 行，连接+资源创建）与 `knowledge-detail-page.tsx`（24 行，键值详情 + 测试连接按钮）——无文档列表、上传、检索测试；
- egress 白名单默认含 lightrag，ragflow 需显式加入（`egress.rs:560-563`）；
- 可复用模式：Skill 上传（multipart + artifacts 表 + `MySqlControlArtifactStore`，`skill_api.rs:645-727、1022-1070`，20MiB/文件上限先例）；MCP 受控调试（`debug-invoke` 确认语义，`mcp_api.rs:561-619`）；Dataset 版本化导入（`dataset_api.rs:409-523`）；e2e fixture `e2e_providers`（LightRAG 集群内实例）。

## 3. 设计

### 3.1 数据模型（Control MySQL 新迁移）

```text
knowledge_documents
  id, tenant_id, rag_resource_id
  name, content_type, size_bytes, sha256
  artifact_id            -- 原件（artifacts 表 + control_objects）
  external_document_id   -- 外部服务文档/批次 ID（LightRAG 返回）
  status                 -- uploading / indexing / indexed / failed
  error_code, error_message
  indexed_at, version, created_by, created_at, updated_at
  唯一键 (tenant_id, rag_resource_id, sha256)   -- 防重复上传同一文档

rag_resources.sync_status 语义激活：有文档在 indexing 时 syncing，全部终态时 synced/failed
```

不建本地 chunks 表——分段预览以 hit-testing 返回的 chunk 为准（外部服务是分段事实源，本地缓存会双源漂移）。

### 3.2 文档上传与索引（LightRAG）

- `POST /api/v1/knowledge/resources/{id}/documents`（multipart，仿 skill 上传）：校验 content_type 白名单与大小上限（首期 8MiB/文件，单资源 200 文档/256MiB 总量）→ 原件入 artifacts → 行 status=uploading → 触发索引；
- 索引执行：Control 侧调用 LightRAG `POST {endpoint}/documents/text`（协议已有：workspace=external_resource_id、indexVersion 注入、`x-api-key`，凭证经 Vault 快照）→ 成功 status=indexed + external_document_id + indexed_at；失败 status=failed + 错误码；
- 索引为同步小任务还是后台任务：**首期同步执行**（文本直传 HTTP，秒级），超时 60s；失败不阻塞其他文档；
- 删除：`DELETE .../documents/{docId}` —— LightRAG 无标准删除协议的边界：首期只删平台记录与 artifact，不回撤外部索引（文档明示"外部索引需在外部服务清理"）；如 LightRAG 提供 `documents/delete` 则调用并以响应为准；
- 列表：`GET .../documents`（分页、状态筛选）。

### 3.3 RAG 协议抽取共享

`rag_query_request`/`finalize_rag_response` 从 runtime crate 抽到共享 crate（`agentx-runtime-contracts` 或 `agentx-node-protocol`，按边界检查工具裁定；倾向 contracts——它是纯协议无执行依赖）：

- Runtime worker 与 Agent 附件槽改为引用共享实现（行为不变，既有协议测试随迁）；
- Control hit-testing 与文档索引复用同一协议构造，避免两处协议漂移。

### 3.4 hit-testing（检索调试）

- `POST /api/v1/knowledge/resources/{id}/retrieval-test`，body `{ query, topK? }`（topK 1..=20）；
- 语义仿 MCP debug-invoke：权限 `knowledge:manage`、记录历史（复用 `resource_health_checks` 模式新表或轻量 `rag_retrieval_tests`：query、topK、命中数、耗时、created_by/created_at——首期直接复用 health 表加 kind 字段亦可，按简单原则定）；
- 执行：Control 直连外部服务（query 协议走共享实现）；结果复用 `rag_execution_output` 归一（documents/citations/recordIds/score），脱敏规则与执行链一致（不记录完整请求体）；
- RAGFlow 同样支持（retrieval 协议已有，external_resource_id 即 dataset_ids）；
- 失败映射：`PROVIDER_UNAVAILABLE`/`PROVIDER_REJECTED` 透出，前端展示原始错误。

### 3.5 前端

`knowledge-detail-page.tsx` 重构为三区（复用 `ResourceDetailLayout` + 现有组件层）：

```text
1 连接与健康：现有键值 + 测试连接（保留）
2 文档管理：文档表（名称/类型/大小/状态/索引时间/错误）+ 上传按钮（拖拽，仿 skill）+
   状态自动轮询（indexing 时）+ 删除（确认 + 影响说明）
   RAGFlow 资源显示"外部自管数据集，文档请在 RAGFlow 侧维护"占位说明
3 检索测试（hit-testing）：query 输入 + topK + 执行按钮 → 结果列表
   （chunk 内容、来源 documentId/chunkId、相似度 score），仿 MCP debug 结果区
```

- i18n 双语词条补齐（文档/上传/索引/状态/hit-testing）；权限沿用 knowledge:view（读）/knowledge:manage（写与测试）。

## 4. 实施阶段

### P7-E1 契约冻结

- [ ] `knowledge_documents` DDL 与文档 API、retrieval-test API 契约（OpenAPI 再生成）；
- [ ] content_type 白名单、大小/数量上限、错误码冻结（`KNOWLEDGE_DOCUMENT_TYPE_UNSUPPORTED`、`KNOWLEDGE_DOCUMENT_TOO_LARGE`、`KNOWLEDGE_DOCUMENT_DUPLICATED`、`KNOWLEDGE_INDEX_FAILED`）；
- [ ] 更新 `docs/05-platform-business.md` 知识库章节与 `docs/13-architecture-service-data-map.md` 表目录。

门禁：契约测试、Schema 测试、boundary check、OpenAPI diff。

### P7-E2 协议抽取

- [ ] RAG 协议（query/insert/finalize）抽到共享 crate，runtime worker 与 Agent 附件槽切换引用；
- [ ] 既有 RAG 协议测试随迁并保持全绿（行为不变重构）。

### P7-E3 文档上传与索引

- [ ] 文档 API（上传/列表/删除）+ artifacts 存储 + 上限校验；
- [ ] LightRAG 索引调用 + 状态回写 + `rag_resources.sync_status` 激活；
- [ ] 凭证与 egress：Control 出网访问 LightRAG 的网络路径确认（Control 面访问 dependencies 命名空间，NetworkPolicy 白名单补规则）。

### P7-E4 hit-testing

- [ ] retrieval-test API（共享协议 + 权限 + 历史/健康记录）；
- [ ] RAGFlow 与 LightRAG 双 provider 联调。

### P7-E5 前端

- [ ] 详情页三区重构 + 上传交互 + 状态轮询 + hit-testing 面板；
- [ ] i18n 双语、深浅主题、空态/错误态；
- [ ] vitest + Playwright knowledge 域用例。

### P7-E6 E2E 验收

- [ ] 见第 5 节。

## 5. E2E 验收（临时 Namespace，复用 P7-D1 的 provider fixture）

1. UI 创建 LightRAG 知识库连接与资源 → 上传 markdown 文档 → 状态 uploading→indexing→indexed，列表正确；
2. 重复上传同 sha256 文档 → `KNOWLEDGE_DOCUMENT_DUPLICATED`；
3. 上传非白名单类型（如 .exe）与超限文件 → 前置校验拒绝；
4. hit-testing 输入查询 → 返回命中 chunk（含刚索引文档内容）、score/documentId 展示正确；
5. 断网/错凭证场景：索引失败状态与错误信息可见，`sync_status=failed`；
6. RAGFlow 资源：文档区显示外部自管说明；hit-testing 对预置 dataset 检索成功；
7. Agent 工作流绑定该知识库资源执行 `knowledge_search` → 引用命中刚上传文档（管理面写入与运行时检索同源验证）；
8. 授权回归：未授权用户不可见/不可测（六态）；
9. UI 覆盖：中英文、深浅主题、上传进度、删除确认。

## 6. 完成定义

- LightRAG 知识库具备上传→索引→状态→hit-testing 完整闭环；RAGFlow 具备 hit-testing 与外部自管说明；
- 上传原件、外部索引、运行时检索三者同源（同一 external_resource_id/workspace 命名空间），Agent 检索可命中管理面上传的文档；
- RAG 协议单源（共享 crate），runtime 与 control 无重复实现；
- 契约测试、单测、Playwright、Kubernetes E2E 全绿；
- `docs/05-platform-business.md` 与实现一致。

## 7. 明确不做

- PDF/Word/HTML 抽取与分段策略配置（后续单独评估，涉及新依赖选型）；
- 本地向量库、embedding 服务、索引管道编排；
- RAGFlow 写路径（协议层面 `RAG_OPERATION_UNSUPPORTED` 语义保持）；
- 检索质量观测（recall 曲线、A/B 召回对比）；
- Dify datasets API 兼容或 DSL 导入。
