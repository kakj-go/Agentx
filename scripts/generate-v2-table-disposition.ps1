$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$catalogPath = Join-Path $root "docs/reference/mysql-schema-catalog.md"
$outputPath = Join-Path $root "docs/planv2/contracts/table-disposition.json"

$catalogTables = Select-String -Path $catalogPath -Pattern '^### ([a-z0-9_]+)$' | ForEach-Object {
    $_.Matches[0].Groups[1].Value
}

$runtime = @(
    "agent_iterations", "agent_runs", "application_invocations", "application_message_parts",
    "application_messages", "application_session_contexts", "application_session_version_history",
    "application_sessions", "checkpoint_artifacts", "checkpoints", "execution_edge_deliveries",
    "execution_end_deliveries", "execution_events", "execution_outbox", "execution_resume_tokens",
    "execution_snapshots", "invocation_events", "item_lineage", "node_attempts", "node_executions",
    "node_invocation_handles", "resume_webhook_bindings", "runtime_calls", "runtime_commands",
    "runtime_idempotency_keys", "runtime_service_heartbeats", "sandbox_leases",
    "side_effect_confirmations", "trigger_bindings", "wait_subscriptions", "worker_capabilities",
    "worker_leases", "workflow_context_patches", "workflow_executions"
)

$split = @(
    "_sqlx_migrations", "application_api_keys", "application_deployment_heads",
    "application_deployments", "application_schedules", "application_webhooks", "applications",
    "approval_actions", "approval_candidates", "approval_tasks", "artifact_references", "artifacts",
    "audit_events", "credential_secret_versions", "credentials", "deployment_history",
    "evaluation_case_results", "evaluation_comparisons", "evaluation_metrics",
    "evaluation_rule_results", "evaluation_run_cases", "evaluation_runs", "notification_receipts",
    "notifications", "outbox_events", "projection_receipts", "quota_policies", "quota_reservations",
    "quota_usage_ledger", "resource_grants", "retention_items", "retention_policies",
    "retention_runs", "tenant_settings", "tenants", "trace_delivery_outbox",
    "workflow_deployment_heads", "workflow_deployments", "workflow_service_identities"
)

$delete = @("release_schema_contract", "trace_delivery_offsets")

$allClassified = @($runtime + $split + $delete)
$duplicates = $allClassified | Group-Object | Where-Object Count -gt 1 | ForEach-Object Name
if ($duplicates.Count -gt 0) {
    throw "Duplicate table decisions: $($duplicates -join ', ')"
}
$unknown = $allClassified | Where-Object { $_ -notin $catalogTables }
if ($unknown.Count -gt 0) {
    throw "Decisions reference unknown tables: $($unknown -join ', ')"
}

function Get-Decision([string]$table) {
    if ($table -in $runtime) { return "runtime" }
    if ($table -in $split) { return "split" }
    if ($table -in $delete) { return "delete" }
    return "control"
}

function Get-Replacements([string]$table, [string]$decision) {
    $known = @{
        "_sqlx_migrations" = @("control._sqlx_migrations", "runtime._sqlx_migrations")
        "tenants" = @("control.tenants", "runtime.tenant_admission")
        "tenant_settings" = @("control.tenant_settings", "runtime.tenant_policy_projection")
        "applications" = @("control.applications", "runtime.application_routes")
        "application_deployments" = @("control.application_deployments", "runtime.deployment_bundles")
        "application_deployment_heads" = @("control.application_deployment_heads", "runtime.deployment_heads")
        "application_api_keys" = @("control.application_api_keys", "runtime.api_key_admission")
        "application_schedules" = @("control.application_schedules", "runtime.schedule_bindings")
        "application_webhooks" = @("control.application_webhooks", "runtime.webhook_bindings")
        "workflow_service_identities" = @("control.workflow_service_identities", "runtime.service_identity_projection")
        "resource_grants" = @("control.resource_grants", "runtime.resource_grant_projection")
        "approval_tasks" = @("control.approval_task_projection", "runtime.approval_tasks")
        "approval_candidates" = @("control.approval_candidate_projection", "runtime.approval_candidates")
        "approval_actions" = @("control.approval_action_submissions", "runtime.approval_decisions")
        "evaluation_runs" = @("control.evaluation_runs", "runtime.evaluation_runs")
        "evaluation_run_cases" = @("control.evaluation_case_projection", "runtime.evaluation_run_cases")
        "artifacts" = @("control.artifacts", "runtime.artifacts")
        "artifact_references" = @("control.artifact_references", "runtime.artifact_references")
        "outbox_events" = @("control.outbox", "runtime.integration_event_log")
        "trace_delivery_outbox" = @("control.trace_delivery_status", "runtime.trace_outbox")
        "projection_receipts" = @("control.projection_receipts", "runtime.command_receipts")
        "quota_policies" = @("control.quota_policies", "runtime.quota_policy_projection")
        "quota_reservations" = @("control.quota_admin_reservations", "runtime.quota_reservations")
        "quota_usage_ledger" = @("control.quota_usage_projection", "runtime.quota_usage_ledger")
        "retention_policies" = @("control.retention_policies", "runtime.retention_policy_projection")
        "retention_runs" = @("control.retention_runs", "runtime.retention_runs")
        "retention_items" = @("control.retention_items", "runtime.retention_items")
        "audit_events" = @("control.audit_events", "runtime.audit_events")
    }
    $values = if ($known.ContainsKey($table)) {
        $known[$table]
    } else {
        switch ($decision) {
            "control" { @("control.$table", $null) }
            "runtime" { @($null, "runtime.$table") }
            "split" { @("control.$table", "runtime.$table") }
            "delete" { @($null, $null) }
        }
    }
    return [pscustomobject]@{ control = $values[0]; runtime = $values[1] }
}

function Get-OwningTask([string]$table, [string]$decision) {
    if ($decision -eq "delete") { return "V2C-001" }
    if ($table -match '^(application_|applications|invocation_|trigger_)') { return "V2G-001..008" }
    if ($table -match '^(evaluation_|dataset_)') { return "V2R-012" }
    if ($table -match '^(approval_|notification)') { return "V2R-010/V2Q-007" }
    if ($table -match '^(artifact|quota_|retention_)') { return "V2R-015" }
    if ($table -match '^(trace_)') { return "V2Q-005" }
    if ($decision -eq "runtime") { return "V2R-001..015" }
    if ($decision -eq "split") { return "V2D-001/V2D-002" }
    return "V2D-001"
}

$records = foreach ($table in $catalogTables) {
    $decision = Get-Decision $table
    $replacement = Get-Replacements $table $decision
    [string[]]$readers = switch ($decision) {
        "control" { "control" }
        "runtime" { "runtime" }
        "split" { "control:control replacement"; "runtime:runtime replacement" }
        "delete" { "none" }
    }
    $contract = if ($decision -eq "split") {
        "versioned Bundle, Admission Command, Runtime Event/Snapshot Export, Receipt or Runtime Query as assigned by the owning task"
    } elseif ($decision -eq "delete") {
        "none; callers must be removed before deletion"
    } else {
        "none; same-plane access only"
    }
    [ordered]@{
        current_table = $table
        decision = $decision
        control_replacement = $replacement.control
        runtime_replacement = $replacement.runtime
        authoritative_writer = switch ($decision) {
            "control" { "control" }
            "runtime" { "runtime" }
            "split" { "control for control replacement; runtime for runtime replacement" }
            "delete" { "none after replacement" }
        }
        allowed_readers = $readers
        cross_plane_contract = $contract
        retention_and_delete_rule = if ($decision -eq "delete") {
            "Delete in V2C-001 after the replacement task and regression evidence pass; no history migration."
        } else {
            "No history migration. Retention is owned by the replacement plane; deletion requires its owning task and reference checks."
        }
        owning_task = Get-OwningTask $table $decision
    }
}

$parent = Split-Path -Parent $outputPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$json = $records | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($outputPath, $json + "`n", [System.Text.UTF8Encoding]::new($false))
Write-Output "Generated $($records.Count) table dispositions at $outputPath"
