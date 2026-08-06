use anyhow::{Context, Result};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, patch, post},
};
use utoipa::OpenApi;

use crate::{
    applications, auth, catalog, connection_test, credentials, datasets, external_resources,
    governance, grants, iam, mcp_control, models::*, models_control, operations,
    runtime_operations, sandbox_profiles, skills_control, state::AppState, workflow_studio,
    workflows,
};

#[derive(OpenApi)]
#[openapi(
    info(title = "Agentx Platform API", version = "1.0.0"),
    paths(
        auth::bootstrap_status, auth::bootstrap, auth::login, auth::refresh, auth::change_password,
        auth::logout, auth::me, iam::list_departments, iam::create_department,
        iam::update_department, iam::delete_department, iam::list_users, iam::create_user,
        iam::update_user, iam::disable_user, iam::list_roles, iam::create_role, iam::update_role,
        iam::list_permissions,
        workflows::list_workflows, workflows::create_workflow, workflows::get_workflow,
        workflows::update_workflow, workflows::archive_workflow, workflows::get_draft,
        workflows::save_draft, workflows::list_revisions, workflows::create_version,
        workflows::list_versions, workflows::list_members, workflows::upsert_member,
        workflows::delete_member, workflows::list_environments, workflows::create_environment,
        workflows::update_environment, workflows::list_deployments, workflows::publish,
        workflows::rollback, workflows::run_workflow,
        catalog::list_node_definitions, catalog::get_node_definition,
        catalog::list_node_provider_options,
        workflow_studio::validate_draft, workflow_studio::preview_expression, workflow_studio::get_debug_overlay,
        workflow_studio::save_debug_overlay, workflow_studio::delete_debug_overlay,
        credentials::list_credentials, credentials::create_credential, credentials::get_credential,
        credentials::update_credential, credentials::rotate_credential,
        models_control::list_models, models_control::get_model, models_control::list_providers,
        models_control::create_provider, models_control::list_deployments,
        models_control::create_deployment, models_control::create_alias, models_control::update_alias,
        models_control::update_provider, models_control::create_deployment_revision,
        models_control::list_deployment_history, models_control::create_price,
        models_control::list_prices, models_control::test_model,
        mcp_control::list_servers, mcp_control::create_server, mcp_control::get_server,
        mcp_control::update_server, mcp_control::test_connection, mcp_control::discover_tools,
        mcp_control::list_tools, mcp_control::list_all_tools, mcp_control::update_tool_policy, mcp_control::debug_invoke,
        skills_control::list_skills, skills_control::create_skill, skills_control::get_skill,
        skills_control::update_skill, skills_control::get_workspace, skills_control::create_entry,
        skills_control::export_workspace, skills_control::import_workspace,
        skills_control::move_entry, skills_control::delete_entry, skills_control::get_file,
        skills_control::update_markdown, skills_control::upload_file,
        skills_control::list_versions, skills_control::create_version,
        external_resources::list_rag_connections, external_resources::create_rag_connection,
        external_resources::test_rag_connection, external_resources::list_knowledge,
        external_resources::create_knowledge, external_resources::get_knowledge,
        external_resources::update_knowledge,
        external_resources::list_memory_connections, external_resources::create_memory_connection,
        external_resources::test_memory_connection, external_resources::list_memory,
        external_resources::create_memory, external_resources::get_memory,
        external_resources::update_memory,
        sandbox_profiles::list_profiles, sandbox_profiles::create_profile,
        sandbox_profiles::get_profile, sandbox_profiles::update_profile,
        sandbox_profiles::create_version,
        grants::list_grantable_resources, grants::list_grants, grants::create_grant, grants::delete_grant,
        grants::validate_workflow_resources,
        applications::list_applications, applications::create_application,
        applications::get_application, applications::update_application,
        applications::list_deployments, applications::create_deployment,
        applications::list_api_keys, applications::create_api_key,
        applications::rotate_api_key, applications::revoke_api_key,
        applications::list_webhooks, applications::create_webhook,
        applications::update_webhook,
        applications::list_schedules, applications::create_schedule,
        applications::update_schedule, applications::list_sessions,
        applications::upgrade_session,
        datasets::list_datasets, datasets::create_dataset, datasets::get_dataset,
        datasets::update_dataset, datasets::list_cases, datasets::create_case,
        datasets::update_case, datasets::delete_case, datasets::import_cases,
        datasets::export_cases, datasets::list_versions, datasets::publish_version,
        datasets::list_profiles, datasets::create_profile,
        datasets::list_evaluations, datasets::create_evaluation,
        datasets::start_evaluation, datasets::cancel_evaluation, datasets::get_report,
        operations::list_approvals, operations::get_approval,
        operations::list_approval_actions, operations::claim_approval,
        operations::list_approval_candidates,
        operations::release_approval, operations::reassign_approval,
        operations::approve, operations::reject, operations::cancel_approval,
        operations::timeout_approval, operations::list_notifications,
        operations::read_notification, operations::read_all_notifications,
        operations::list_executions, operations::get_execution,
        operations::execution_trace, operations::execution_artifact, operations::execution_runtime_details, operations::runtime_status,
        operations::dashboard_summary,
        runtime_operations::start_execution, runtime_operations::cancel_execution,
        runtime_operations::start_debug_execution, runtime_operations::list_execution_events,
        runtime_operations::list_nodes, runtime_operations::get_node,
        runtime_operations::list_checkpoints, runtime_operations::list_waits,
        runtime_operations::fork_execution, runtime_operations::confirm_side_effect,
        governance::get_quotas, governance::update_quotas, governance::get_capabilities,
        governance::create_retention_run, governance::list_retention_runs,
        governance::list_retention_items
    ),
    components(schemas(
        BootstrapStatus, BootstrapRequest, LoginRequest, ChangePasswordRequest, AuthResponse,
        MeResponse, DepartmentResponse, CreateDepartmentRequest, UpdateDepartmentRequest,
        UserResponse, CreateUserRequest, UpdateUserRequest, RoleResponse, CreateRoleRequest,
        UpdateRoleRequest, PermissionResponse, agentx_api_types::ApiErrorResponse,
        agentx_api_types::FieldError,
        agentx_api_types::HealthResponse, agentx_api_types::DependencyHealth
        ,workflows::WorkflowResponse, workflows::CreateWorkflowRequest,
        workflows::UpdateWorkflowRequest, workflows::DraftResponse, workflows::SaveDraftRequest,
        workflows::RevisionResponse, workflows::WorkflowVersionResponse,
        workflows::CreateVersionRequest, workflows::WorkflowMemberResponse,
        workflows::UpsertWorkflowMemberRequest, workflows::EnvironmentResponse,
        workflows::CreateEnvironmentRequest, workflows::UpdateEnvironmentRequest,
        workflows::DeploymentResponse,
        workflows::PublishWorkflowRequest, workflows::QueuedWorkflowRunResponse,
        workflows::RollbackWorkflowRequest, workflows::RunWorkflowRequest,
        catalog::NodeDefinitionQuery, catalog::NodeDefinitionSummary,
        catalog::NodeDefinitionDetail, catalog::NodeProviderQuery,
        catalog::NodeProviderOption, catalog::NodeProviderOptionsResponse,
        workflow_studio::ValidateDraftRequest, workflow_studio::ValidationIssue,
        workflow_studio::ValidateDraftResponse, workflow_studio::ExpressionPreviewRequest,
        workflow_studio::ExpressionPreviewResponse, workflow_studio::DebugOverlayResponse,
        workflow_studio::SaveDebugOverlayRequest,
        credentials::CredentialResponse, credentials::CreateCredentialRequest,
        credentials::UpdateCredentialRequest, credentials::RotateCredentialRequest,
        models_control::ModelResponse, models_control::ModelProviderResponse,
        models_control::CreateModelProviderRequest, models_control::ModelDeploymentResponse,
        models_control::CreateModelDeploymentRequest, models_control::CreateModelAliasRequest,
        models_control::UpdateModelAliasRequest, models_control::ModelPriceResponse,
        models_control::CreateModelPriceRequest, models_control::UpdateModelProviderRequest,
        models_control::CreateDeploymentRevisionRequest,
        models_control::ModelDeploymentHistoryResponse,
        mcp_control::McpServerResponse, mcp_control::CreateMcpServerRequest,
        mcp_control::UpdateMcpServerRequest, mcp_control::McpToolResponse,
        mcp_control::UpdateMcpToolPolicyRequest, mcp_control::DebugMcpToolRequest,
        mcp_control::DebugMcpToolResponse, mcp_control::McpDiscoveryResponse,
        skills_control::SkillResponse, skills_control::CreateSkillRequest,
        skills_control::UpdateSkillRequest, skills_control::SkillVersionResponse,
        skills_control::SkillDependencyInput, skills_control::SkillWorkspaceEntry,
        skills_control::SkillWorkspaceResponse, skills_control::CreateEntryRequest,
        skills_control::MoveEntryRequest, skills_control::UpdateMarkdownRequest,
        skills_control::PublishSkillVersionRequest, skills_control::ArtifactUploadResponse,
        external_resources::ConnectionResponse, external_resources::CreateConnectionRequest,
        external_resources::KnowledgeResponse, external_resources::CreateKnowledgeRequest,
        external_resources::MemoryResponse, external_resources::CreateMemoryRequest,
        external_resources::UpdateExternalResourceRequest,
        sandbox_profiles::SandboxProfileResponse,
        sandbox_profiles::SandboxProfileVersionResponse,
        sandbox_profiles::SandboxProfileVersionInput,
        sandbox_profiles::CreateSandboxProfileRequest,
        sandbox_profiles::UpdateSandboxProfileRequest,
        connection_test::HealthCheckResponse, grants::GrantableResourceResponse, grants::GrantResponse,
        grants::CreateGrantRequest, grants::ResourceValidationResponse,
        grants::MissingGrantResponse,
        applications::ApplicationResponse, applications::CreateApplicationRequest,
        applications::UpdateApplicationRequest, applications::ApplicationDeploymentResponse,
        applications::CreateApplicationDeploymentRequest, applications::ApiKeyResponse,
        applications::CreateApiKeyRequest, applications::WebhookResponse,
        applications::CreateWebhookRequest, applications::ScheduleResponse,
        applications::UpdateWebhookRequest,
        applications::CreateScheduleRequest, applications::UpdateScheduleRequest,
        applications::SessionResponse, applications::UpgradeSessionRequest,
        datasets::DatasetResponse, datasets::CreateDatasetRequest,
        datasets::UpdateDatasetRequest, datasets::TestCaseResponse, datasets::CaseInput,
        datasets::CreateCaseRequest, datasets::UpdateCaseRequest,
        datasets::DeleteCaseRequest, datasets::ImportCasesRequest,
        datasets::DatasetVersionResponse, datasets::EvaluationRuleInput,
        datasets::EvaluationRuleResponse, datasets::EvaluationProfileResponse,
        datasets::CreateEvaluationProfileRequest, datasets::EvaluationRunResponse,
        datasets::CreateEvaluationRunRequest, datasets::EvaluationCaseResultResponse,
        datasets::EvaluationReportResponse, datasets::EvaluationRuleResultResponse,
        operations::ApprovalResponse, operations::VersionActionRequest,
        operations::ReassignApprovalRequest, operations::DecideApprovalRequest,
        operations::ApprovalActionResponse, operations::NotificationResponse,
        operations::ApprovalCandidateResponse,
        operations::NotificationInboxResponse, operations::ExecutionResponse,
        operations::TraceEventResponse, operations::TraceResponse,
        operations::RuntimeComponentStatus, operations::RuntimeStatusResponse,
        operations::RuntimeDetailsResponse, operations::AgentRunDetail,
        operations::AgentIterationDetail, operations::RuntimeCallDetail, operations::SandboxLeaseDetail,
        operations::DashboardSummaryResponse,
        runtime_operations::StartExecutionRequest, runtime_operations::ExecutionCommandResponse,
        runtime_operations::NodeAttemptResponse, runtime_operations::LineageResponse,
        runtime_operations::NodeExecutionResponse, runtime_operations::NodeExecutionListResponse,
        runtime_operations::CheckpointResponse, runtime_operations::CheckpointListResponse,
        runtime_operations::WaitResponse, runtime_operations::WaitListResponse,
        runtime_operations::ForkRequest, runtime_operations::SideEffectConfirmationRequest,
        runtime_operations::IdempotentCommandResponse, runtime_operations::DebugExecutionRequest,
        runtime_operations::ExecutionEventQuery, runtime_operations::ExecutionEventResponse,
        runtime_operations::ExecutionEventListResponse,
        governance::QuotaPolicyInput, governance::UpdateQuotaPoliciesRequest,
        governance::QuotaPolicyResponse, governance::WorkerCapabilityResponse,
        governance::CreateRetentionRunRequest, governance::RetentionRunResponse,
        governance::RetentionItemResponse
    )),
    tags(
        (name = "Agentx M1", description = "Bootstrap, authentication and IAM control plane"),
        (name = "Agentx M2", description = "Workflow and resource control plane"),
        (name = "Agentx M3", description = "Application, evaluation and operations control plane")
    )
)]
struct ApiDoc;

pub(crate) fn openapi_json() -> Result<String> {
    serde_json::to_string_pretty(&ApiDoc::openapi()).context("failed to serialize OpenAPI")
}

pub(crate) fn build_api_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/bootstrap/status", get(auth::bootstrap_status))
        .route("/bootstrap", post(auth::bootstrap))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/change-password", post(auth::change_password))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route(
            "/departments",
            get(iam::list_departments).post(iam::create_department),
        )
        .route(
            "/departments/{id}",
            patch(iam::update_department).delete(iam::delete_department),
        )
        .route("/users", get(iam::list_users).post(iam::create_user))
        .route("/users/{id}", patch(iam::update_user))
        .route("/users/{id}/disable", post(iam::disable_user))
        .route("/roles", get(iam::list_roles).post(iam::create_role))
        .route("/roles/{id}", patch(iam::update_role))
        .route("/permissions", get(iam::list_permissions));
    let api = api
        .route("/node-definitions", get(catalog::list_node_definitions))
        .route(
            "/node-definitions/{node_type}/versions/{version}",
            get(catalog::get_node_definition),
        )
        .route(
            "/node-definitions/{node_type}/versions/{version}/providers/{provider}",
            get(catalog::list_node_provider_options),
        )
        .route(
            "/sandbox-profiles",
            get(sandbox_profiles::list_profiles).post(sandbox_profiles::create_profile),
        )
        .route(
            "/sandbox-profiles/{id}",
            get(sandbox_profiles::get_profile).patch(sandbox_profiles::update_profile),
        )
        .route(
            "/sandbox-profiles/{id}/versions",
            post(sandbox_profiles::create_version),
        );
    let api = api
        .route(
            "/workflows",
            get(workflows::list_workflows).post(workflows::create_workflow),
        )
        .route(
            "/workflows/{id}",
            get(workflows::get_workflow).patch(workflows::update_workflow),
        )
        .route("/workflows/{id}/archive", post(workflows::archive_workflow))
        .route(
            "/workflows/{id}/draft",
            get(workflows::get_draft).put(workflows::save_draft),
        )
        .route(
            "/workflows/{id}/draft/validate",
            post(workflow_studio::validate_draft),
        )
        .route(
            "/workflows/{id}/expressions/preview",
            post(workflow_studio::preview_expression),
        )
        .route(
            "/workflows/{id}/debug-overlays/{node_id}",
            get(workflow_studio::get_debug_overlay)
                .put(workflow_studio::save_debug_overlay)
                .delete(workflow_studio::delete_debug_overlay),
        )
        .route("/workflows/{id}/revisions", get(workflows::list_revisions))
        .route(
            "/workflows/{id}/versions",
            get(workflows::list_versions).post(workflows::create_version),
        )
        .route(
            "/workflows/{id}/members",
            get(workflows::list_members).post(workflows::upsert_member),
        )
        .route(
            "/workflows/{id}/members/{user_id}",
            axum::routing::delete(workflows::delete_member),
        )
        .route(
            "/workflows/{id}/deployments",
            get(workflows::list_deployments).post(workflows::publish),
        )
        .route(
            "/workflows/{id}/deployments/{environment_id}/rollback",
            post(workflows::rollback),
        )
        .route(
            "/workflows/{id}/resource-validation",
            get(grants::validate_workflow_resources),
        )
        .route("/workflows/{id}/run", post(workflows::run_workflow))
        .route(
            "/workflows/{id}/debug-executions",
            post(runtime_operations::start_debug_execution),
        )
        .route(
            "/environments",
            get(workflows::list_environments).post(workflows::create_environment),
        )
        .route("/environments/{id}", patch(workflows::update_environment))
        .route(
            "/credentials",
            get(credentials::list_credentials).post(credentials::create_credential),
        )
        .route(
            "/credentials/{id}",
            get(credentials::get_credential).patch(credentials::update_credential),
        )
        .route(
            "/credentials/{id}/rotate",
            post(credentials::rotate_credential),
        )
        .route(
            "/models/aliases",
            get(models_control::list_models).post(models_control::create_alias),
        )
        .route(
            "/models/aliases/{id}",
            get(models_control::get_model).patch(models_control::update_alias),
        )
        .route(
            "/models/providers",
            get(models_control::list_providers).post(models_control::create_provider),
        )
        .route(
            "/models/providers/{id}",
            patch(models_control::update_provider),
        )
        .route(
            "/models/aliases/{id}/test-connection",
            post(models_control::test_model),
        )
        .route(
            "/models/deployments",
            get(models_control::list_deployments).post(models_control::create_deployment),
        )
        .route(
            "/models/deployments/{id}/prices",
            get(models_control::list_prices).post(models_control::create_price),
        )
        .route(
            "/models/aliases/{id}/deployment-revisions",
            post(models_control::create_deployment_revision),
        )
        .route(
            "/models/aliases/{id}/deployment-history",
            get(models_control::list_deployment_history),
        )
        .route(
            "/mcp/servers",
            get(mcp_control::list_servers).post(mcp_control::create_server),
        )
        .route(
            "/mcp/servers/{id}",
            get(mcp_control::get_server).patch(mcp_control::update_server),
        )
        .route(
            "/mcp/servers/{id}/test-connection",
            post(mcp_control::test_connection),
        )
        .route(
            "/mcp/servers/{id}/discover",
            post(mcp_control::discover_tools),
        )
        .route("/mcp/servers/{id}/tools", get(mcp_control::list_tools))
        .route("/mcp/tools", get(mcp_control::list_all_tools))
        .route(
            "/mcp/tools/{id}/policy",
            patch(mcp_control::update_tool_policy),
        )
        .route(
            "/mcp/tools/{id}/debug-invoke",
            post(mcp_control::debug_invoke),
        )
        .route(
            "/skills",
            get(skills_control::list_skills).post(skills_control::create_skill),
        )
        .route(
            "/skills/{id}",
            get(skills_control::get_skill).patch(skills_control::update_skill),
        )
        .route("/skills/{id}/workspace", get(skills_control::get_workspace))
        .route(
            "/skills/{id}/workspace/export",
            get(skills_control::export_workspace),
        )
        .route(
            "/skills/{id}/workspace/import",
            post(skills_control::import_workspace),
        )
        .route("/skills/{id}/entries", post(skills_control::create_entry))
        .route(
            "/skills/{id}/entries/{entry_id}",
            patch(skills_control::move_entry).delete(skills_control::delete_entry),
        )
        .route(
            "/skills/{id}/files/{entry_id}",
            get(skills_control::get_file).put(skills_control::update_markdown),
        )
        .route("/skills/{id}/uploads", post(skills_control::upload_file))
        .route(
            "/skills/{id}/versions",
            get(skills_control::list_versions).post(skills_control::create_version),
        )
        .route(
            "/knowledge/connections",
            get(external_resources::list_rag_connections)
                .post(external_resources::create_rag_connection),
        )
        .route(
            "/knowledge/connections/{id}/test-connection",
            post(external_resources::test_rag_connection),
        )
        .route(
            "/knowledge/resources",
            get(external_resources::list_knowledge).post(external_resources::create_knowledge),
        )
        .route(
            "/knowledge/resources/{id}",
            get(external_resources::get_knowledge).patch(external_resources::update_knowledge),
        )
        .route(
            "/memory/connections",
            get(external_resources::list_memory_connections)
                .post(external_resources::create_memory_connection),
        )
        .route(
            "/memory/connections/{id}/test-connection",
            post(external_resources::test_memory_connection),
        )
        .route(
            "/memory/namespaces",
            get(external_resources::list_memory).post(external_resources::create_memory),
        )
        .route(
            "/memory/namespaces/{id}",
            get(external_resources::get_memory).patch(external_resources::update_memory),
        )
        .route(
            "/resources/grantable",
            get(grants::list_grantable_resources),
        )
        .route(
            "/resources/{resource_type}/{resource_id}/grants",
            get(grants::list_grants).post(grants::create_grant),
        )
        .route(
            "/resources/{resource_type}/{resource_id}/grants/{grant_id}",
            axum::routing::delete(grants::delete_grant),
        )
        .route(
            "/applications",
            get(applications::list_applications).post(applications::create_application),
        )
        .route(
            "/applications/{id}",
            get(applications::get_application).patch(applications::update_application),
        )
        .route(
            "/applications/{id}/deployments",
            get(applications::list_deployments).post(applications::create_deployment),
        )
        .route(
            "/applications/{id}/api-keys",
            get(applications::list_api_keys).post(applications::create_api_key),
        )
        .route(
            "/applications/{id}/api-keys/{key_id}/rotate",
            post(applications::rotate_api_key),
        )
        .route(
            "/applications/{id}/api-keys/{key_id}/revoke",
            post(applications::revoke_api_key),
        )
        .route(
            "/applications/{id}/webhooks",
            get(applications::list_webhooks).post(applications::create_webhook),
        )
        .route(
            "/applications/{id}/webhooks/{webhook_id}",
            patch(applications::update_webhook),
        )
        .route(
            "/applications/{id}/schedules",
            get(applications::list_schedules).post(applications::create_schedule),
        )
        .route(
            "/applications/{id}/schedules/{schedule_id}",
            patch(applications::update_schedule),
        )
        .route(
            "/applications/{id}/sessions",
            get(applications::list_sessions),
        )
        .route(
            "/sessions/{id}/upgrade",
            post(applications::upgrade_session),
        )
        .route(
            "/datasets",
            get(datasets::list_datasets).post(datasets::create_dataset),
        )
        .route(
            "/datasets/{id}",
            get(datasets::get_dataset).patch(datasets::update_dataset),
        )
        .route(
            "/datasets/{id}/cases",
            get(datasets::list_cases).post(datasets::create_case),
        )
        .route(
            "/datasets/{id}/cases/{case_id}",
            patch(datasets::update_case).delete(datasets::delete_case),
        )
        .route("/datasets/{id}/import", post(datasets::import_cases))
        .route("/datasets/{id}/export", get(datasets::export_cases))
        .route(
            "/datasets/{id}/versions",
            get(datasets::list_versions).post(datasets::publish_version),
        )
        .route(
            "/evaluation-profiles",
            get(datasets::list_profiles).post(datasets::create_profile),
        )
        .route(
            "/evaluations",
            get(datasets::list_evaluations).post(datasets::create_evaluation),
        )
        .route("/evaluations/{id}/start", post(datasets::start_evaluation))
        .route(
            "/evaluations/{id}/cancel",
            post(datasets::cancel_evaluation),
        )
        .route("/evaluations/{id}/report", get(datasets::get_report))
        .route("/approvals", get(operations::list_approvals))
        .route("/approvals/{id}", get(operations::get_approval))
        .route(
            "/approvals/{id}/actions",
            get(operations::list_approval_actions),
        )
        .route(
            "/approvals/{id}/candidates",
            get(operations::list_approval_candidates),
        )
        .route("/approvals/{id}/claim", post(operations::claim_approval))
        .route(
            "/approvals/{id}/release",
            post(operations::release_approval),
        )
        .route(
            "/approvals/{id}/reassign",
            post(operations::reassign_approval),
        )
        .route("/approvals/{id}/approve", post(operations::approve))
        .route("/approvals/{id}/reject", post(operations::reject))
        .route("/approvals/{id}/cancel", post(operations::cancel_approval))
        .route(
            "/approvals/{id}/timeout",
            post(operations::timeout_approval),
        )
        .route("/notifications", get(operations::list_notifications))
        .route(
            "/notifications/read-all",
            post(operations::read_all_notifications),
        )
        .route(
            "/notifications/{id}/read",
            post(operations::read_notification),
        )
        .route("/executions", get(operations::list_executions))
        .route("/executions/{id}", get(operations::get_execution))
        .route(
            "/executions/{id}/runtime-details",
            get(operations::execution_runtime_details),
        )
        .route(
            "/workflow-versions/{version_id}/executions",
            post(runtime_operations::start_execution),
        )
        .route(
            "/executions/{id}/cancel",
            post(runtime_operations::cancel_execution),
        )
        .route(
            "/executions/{id}/events",
            get(runtime_operations::list_execution_events),
        )
        .route(
            "/executions/{id}/nodes",
            get(runtime_operations::list_nodes),
        )
        .route(
            "/executions/{id}/nodes/{node_execution_id}",
            get(runtime_operations::get_node),
        )
        .route(
            "/executions/{id}/checkpoints",
            get(runtime_operations::list_checkpoints),
        )
        .route(
            "/executions/{id}/waits",
            get(runtime_operations::list_waits),
        )
        .route(
            "/executions/{id}/fork",
            post(runtime_operations::fork_execution),
        )
        .route(
            "/executions/{id}/side-effect-confirmations",
            post(runtime_operations::confirm_side_effect),
        )
        .route("/executions/{id}/trace", get(operations::execution_trace))
        .route(
            "/executions/{id}/artifacts/{artifact_id}",
            get(operations::execution_artifact),
        )
        .route("/runtime/status", get(operations::runtime_status))
        .route(
            "/runtime/quotas",
            get(governance::get_quotas).put(governance::update_quotas),
        )
        .route("/runtime/capabilities", get(governance::get_capabilities))
        .route(
            "/retention-runs",
            get(governance::list_retention_runs).post(governance::create_retention_run),
        )
        .route(
            "/retention-runs/{id}/items",
            get(governance::list_retention_items),
        )
        .route("/dashboard/summary", get(operations::dashboard_summary));
    Router::new()
        .route(
            "/internal/v1/credentials/resolve",
            post(credentials::broker_resolve),
        )
        .route(
            "/internal/v1/webhooks/resolve",
            post(credentials::broker_resolve_webhook),
        )
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(105 * 1024 * 1024))
        .with_state(state)
}
