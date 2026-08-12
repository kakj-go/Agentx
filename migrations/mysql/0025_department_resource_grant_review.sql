INSERT IGNORE INTO role_permissions(tenant_id, role_id, permission_id)
SELECT r.tenant_id, r.id, p.id
FROM roles r
JOIN permissions p ON p.permission_key = 'resource:grant'
WHERE r.code = 'department_admin';
