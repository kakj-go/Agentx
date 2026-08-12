CREATE TABLE workflow_draft_resources (
    id BINARY(16) NOT NULL,
    tenant_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    draft_id BINARY(16) NOT NULL,
    node_id VARCHAR(128) NOT NULL,
    node_name VARCHAR(160) NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    resource_version_id BINARY(16) NULL,
    operation_key VARCHAR(32) NOT NULL,
    relation VARCHAR(64) NOT NULL DEFAULT 'resource_reference',
    created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uq_workflow_draft_resource (
        draft_id, node_id, resource_type, resource_id, operation_key, relation
    ),
    KEY idx_workflow_draft_resources_target (
        tenant_id, resource_type, resource_id, workflow_id
    ),
    KEY idx_workflow_draft_resources_workflow (tenant_id, workflow_id, node_id),
    CONSTRAINT fk_workflow_draft_resource_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    CONSTRAINT fk_workflow_draft_resource_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id),
    CONSTRAINT fk_workflow_draft_resource_draft FOREIGN KEY (draft_id) REFERENCES workflow_drafts(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

INSERT INTO workflow_draft_resources (
    id, tenant_id, workflow_id, draft_id, node_id, node_name, resource_type,
    resource_id, resource_version_id, operation_key, relation
)
SELECT UUID_TO_BIN(UUID()), d.tenant_id, d.workflow_id, d.id, nodes.node_id,
       nodes.node_name, refs.resource_type, UUID_TO_BIN(refs.resource_id),
       IF(refs.resource_version_id IS NULL, NULL, UUID_TO_BIN(refs.resource_version_id)),
       refs.operation_key, 'resource_reference'
FROM workflow_drafts d
JOIN JSON_TABLE(d.definition_json, '$.nodes[*]' COLUMNS (
    node_id VARCHAR(128) PATH '$.id',
    node_name VARCHAR(160) PATH '$.name',
    resource_references JSON PATH '$.resourceReferences'
)) nodes
JOIN JSON_TABLE(nodes.resource_references, '$[*]' COLUMNS (
    resource_type VARCHAR(32) PATH '$.resourceType',
    resource_id CHAR(36) PATH '$.resourceId',
    resource_version_id CHAR(36) PATH '$.resourceVersionId' NULL ON EMPTY,
    operation_key VARCHAR(32) PATH '$.operation'
)) refs;

INSERT IGNORE INTO workflow_draft_resources (
    id, tenant_id, workflow_id, draft_id, node_id, node_name, resource_type,
    resource_id, resource_version_id, operation_key, relation
)
SELECT UUID_TO_BIN(UUID()), d.tenant_id, d.workflow_id, d.id, nodes.node_id,
       nodes.node_name, 'workflow', v.workflow_id, UUID_TO_BIN(nodes.workflow_version_text),
       'use', 'subworkflow'
FROM workflow_drafts d
JOIN JSON_TABLE(d.definition_json, '$.nodes[*]' COLUMNS (
    node_id VARCHAR(128) PATH '$.id',
    node_name VARCHAR(160) PATH '$.name',
    node_type VARCHAR(128) PATH '$.type',
    workflow_version_text CHAR(36) PATH '$.parameters.workflowVersionId' NULL ON EMPTY
)) nodes
JOIN workflow_versions v
  ON v.tenant_id=d.tenant_id
 AND v.id=UUID_TO_BIN(nodes.workflow_version_text)
WHERE nodes.node_type='sub_workflow' OR nodes.node_type LIKE 'workflow.%';

INSERT INTO permissions (id, permission_key, name) VALUES
    (UUID_TO_BIN(UUID()), 'workflow:delete', 'Delete workflows and environments'),
    (UUID_TO_BIN(UUID()), 'application:delete', 'Delete applications, webhooks, and schedules'),
    (UUID_TO_BIN(UUID()), 'credential:delete', 'Delete credentials'),
    (UUID_TO_BIN(UUID()), 'model:delete', 'Delete models'),
    (UUID_TO_BIN(UUID()), 'mcp:delete', 'Delete MCP servers'),
    (UUID_TO_BIN(UUID()), 'skill:delete', 'Delete skills'),
    (UUID_TO_BIN(UUID()), 'knowledge:delete', 'Delete knowledge resources'),
    (UUID_TO_BIN(UUID()), 'memory:delete', 'Delete memory resources'),
    (UUID_TO_BIN(UUID()), 'sandbox:delete', 'Delete Sandbox Profiles'),
    (UUID_TO_BIN(UUID()), 'dataset:delete', 'Delete datasets'),
    (UUID_TO_BIN(UUID()), 'evaluation_profile:delete', 'Delete evaluation profiles'),
    (UUID_TO_BIN(UUID()), 'department:delete', 'Delete departments'),
    (UUID_TO_BIN(UUID()), 'role:delete', 'Delete roles')
ON DUPLICATE KEY UPDATE name = VALUES(name);

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, delete_permission.id
FROM roles r
JOIN role_permissions existing ON existing.role_id = r.id
JOIN permissions manage_permission ON manage_permission.id = existing.permission_id
JOIN permissions delete_permission ON delete_permission.permission_key = CASE manage_permission.permission_key
    WHEN 'workflow:archive' THEN 'workflow:delete'
    WHEN 'application:manage' THEN 'application:delete'
    WHEN 'credential:manage' THEN 'credential:delete'
    WHEN 'model:manage' THEN 'model:delete'
    WHEN 'mcp:manage' THEN 'mcp:delete'
    WHEN 'skill:manage' THEN 'skill:delete'
    WHEN 'knowledge:manage' THEN 'knowledge:delete'
    WHEN 'memory:manage' THEN 'memory:delete'
    WHEN 'sandbox:manage' THEN 'sandbox:delete'
    WHEN 'dataset:manage' THEN 'dataset:delete'
    WHEN 'evaluation_profile:manage' THEN 'evaluation_profile:delete'
    WHEN 'department:manage' THEN 'department:delete'
    WHEN 'role:manage' THEN 'role:delete'
END
WHERE r.is_builtin = TRUE
  AND manage_permission.permission_key IN (
      'workflow:archive', 'application:manage', 'credential:manage', 'model:manage',
      'mcp:manage', 'skill:manage', 'knowledge:manage', 'memory:manage',
      'sandbox:manage', 'dataset:manage', 'evaluation_profile:manage',
      'department:manage', 'role:manage'
  );

INSERT IGNORE INTO role_permissions (tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code = 'company_admin'
  AND p.permission_key IN (
      'workflow:delete', 'application:delete', 'credential:delete', 'model:delete',
      'mcp:delete', 'skill:delete', 'knowledge:delete', 'memory:delete',
      'sandbox:delete', 'dataset:delete', 'evaluation_profile:delete',
      'department:delete', 'role:delete'
  );
