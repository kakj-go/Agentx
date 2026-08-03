mod auth;
mod config;
mod connection_test;
mod control_common;
mod credentials;
mod error;
mod external_resources;
mod grants;
mod iam;
mod mcp_control;
mod models;
mod models_control;
mod security;
mod skills_control;
mod state;
mod workflows;

use std::{env, time::Duration};

use agentx_infrastructure::{config::InfrastructureSettings, credential::CredentialKeyring, mysql};
use anyhow::{Context, Result};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, patch, post},
};
use config::{AuthSettings, ConnectionSettings, CredentialSettings};
use models::*;
use state::AppState;
use utoipa::OpenApi;

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
        workflows::rollback, workflows::runtime_unavailable,
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
        grants::list_grantable_resources, grants::list_grants, grants::create_grant, grants::delete_grant,
        grants::validate_workflow_resources
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
        workflows::PublishWorkflowRequest, workflows::RollbackWorkflowRequest,
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
        connection_test::HealthCheckResponse, grants::GrantableResourceResponse, grants::GrantResponse,
        grants::CreateGrantRequest, grants::ResourceValidationResponse,
        grants::MissingGrantResponse
    )),
    tags(
        (name = "Agentx M1", description = "Bootstrap, authentication and IAM control plane"),
        (name = "Agentx M2", description = "Workflow and resource control plane")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<()> {
    let command = env::args().nth(1);
    if command.as_deref() == Some("openapi") {
        let path = env::args()
            .nth(2)
            .unwrap_or_else(|| "openapi/platform-api.json".to_owned());
        let content = serde_json::to_string_pretty(&ApiDoc::openapi())
            .context("failed to serialize OpenAPI")?;
        std::fs::write(&path, format!("{content}\n"))
            .with_context(|| format!("failed to write {path}"))?;
        return Ok(());
    }

    let infrastructure = InfrastructureSettings::from_env()?;
    let pool = mysql::connect(&infrastructure.mysql).await?;
    if command.as_deref() == Some("migrate") {
        mysql::run_migrations(&pool).await?;
        return Ok(());
    }
    let auth = AuthSettings::from_env()?;
    let credential = CredentialSettings::from_env()?;
    let keyring = CredentialKeyring::from_json(credential.active_key_id, &credential.keys_json)?;
    let object_store =
        agentx_infrastructure::clients::object_store(&infrastructure.object_storage).ok();
    let state = AppState::new(pool.clone(), auth).with_m2(
        keyring,
        object_store,
        ConnectionSettings::from_env()?,
    );
    let health = agentx_service_kit::HealthRegistry::default();
    health.register("mysql", true).await;
    health.register("redis", false).await;
    health.register("clickhouse", false).await;
    health.register("object_storage", false).await;
    start_health_checks(health.clone(), infrastructure, pool);

    let router = build_api_router(state);
    agentx_service_kit::serve("platform-api", router, health).await
}

fn build_api_router(state: AppState) -> Router {
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
        .route("/workflows/{id}/run", post(workflows::runtime_unavailable))
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
        );
    Router::new()
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(105 * 1024 * 1024))
        .with_state(state)
}

fn start_health_checks(
    registry: agentx_service_kit::HealthRegistry,
    settings: InfrastructureSettings,
    pool: sqlx::MySqlPool,
) {
    let object_store = agentx_infrastructure::clients::object_store(&settings.object_storage).ok();
    tokio::spawn(async move {
        use futures::StreamExt;
        loop {
            let schema_ready = sqlx::query_scalar::<_, bool>(
                "SELECT COALESCE(MAX(version),0) >= 7 AND COALESCE(MIN(success),0)=1 FROM _sqlx_migrations",
            ).fetch_one(&pool).await.unwrap_or(false);
            let mysql_ready = mysql::ping(&pool).await.is_ok() && schema_ready;
            registry
                .set_status("mysql", if mysql_ready { "ready" } else { "unavailable" })
                .await;
            let redis_ready =
                match agentx_infrastructure::clients::connect_redis(&settings.redis).await {
                    Ok(mut connection) => redis::cmd("PING")
                        .query_async::<String>(&mut connection)
                        .await
                        .is_ok(),
                    Err(_) => false,
                };
            registry
                .set_status("redis", if redis_ready { "ready" } else { "degraded" })
                .await;
            let clickhouse_ready = agentx_infrastructure::clients::clickhouse(&settings.clickhouse)
                .query("SELECT 1")
                .fetch_one::<u8>()
                .await
                .is_ok();
            registry
                .set_status(
                    "clickhouse",
                    if clickhouse_ready {
                        "ready"
                    } else {
                        "degraded"
                    },
                )
                .await;
            let object_ready = if let Some(store) = &object_store {
                match store.list(None).next().await {
                    Some(Ok(_)) | None => true,
                    Some(Err(_)) => false,
                }
            } else {
                false
            };
            registry
                .set_status(
                    "object_storage",
                    if object_ready { "ready" } else { "degraded" },
                )
                .await;
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

#[cfg(test)]
mod integration_tests {
    use agentx_application::{
        ArtifactStore, ArtifactWrite, Outbox, OutboxDispatcher, OutboxMessage,
    };
    use agentx_domain::TenantId;
    use agentx_infrastructure::{
        clients,
        config::{ClickHouseSettings, MySqlSettings, RedisSettings},
        credential::CredentialKeyring,
        mysql,
    };
    use object_store::memory::InMemory;
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use std::{borrow::Cow, path::Path, sync::Arc, time::Duration};
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use tower::ServiceExt;

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
        routing::get,
    };

    use crate::{
        config::{AuthSettings, ConnectionSettings},
        state::AppState,
    };

    fn auth_settings() -> AuthSettings {
        AuthSettings {
            signing_secret: SecretString::from(
                "a-development-secret-with-more-than-32-characters".to_owned(),
            ),
            issuer: "agentx-test".to_owned(),
            audience: "agentx-test-web".to_owned(),
            access_ttl_seconds: 900,
            refresh_ttl_seconds: 604_800,
            change_password_ttl_seconds: 600,
            cookie_secure: false,
            login_max_failures: 2,
            login_failure_window_seconds: 900,
            login_lock_seconds: 900,
        }
    }

    fn json_request(method: &str, uri: &str, body: Value, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder
            .body(Body::from(body.to_string()))
            .expect("valid test request")
    }

    fn idempotent_json_request(
        method: &str,
        uri: &str,
        body: Value,
        token: &str,
        key: &str,
    ) -> Request<Body> {
        let mut request = json_request(method, uri, body, Some(token));
        request.headers_mut().insert(
            "idempotency-key",
            key.parse().expect("valid idempotency header"),
        );
        request
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read response body");
        serde_json::from_slice(&body).expect("JSON response")
    }

    #[tokio::test]
    async fn mysql_migrations_are_idempotent_on_an_empty_database() {
        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .expect("MySQL container should start");
        let port = container
            .get_host_port_ipv4(3306.tcp())
            .await
            .expect("mapped MySQL port");
        let settings = MySqlSettings {
            host: "127.0.0.1".to_owned(),
            port,
            database: "agentx".to_owned(),
            username: "agentx".to_owned(),
            password: SecretString::from("agentx-test-password".to_owned()),
            max_connections: 5,
        };
        let mut pool = None;
        for _ in 0..30 {
            match mysql::connect(&settings).await {
                Ok(value) => {
                    pool = Some(value);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
        let pool = pool.expect("connect to test MySQL");
        mysql::run_migrations(&pool)
            .await
            .expect("first migration run");
        mysql::run_migrations(&pool)
            .await
            .expect("second migration run");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bootstrap_state")
            .fetch_one(&pool)
            .await
            .expect("bootstrap row");
        assert_eq!(count, 1);

        let keyring = CredentialKeyring::from_json(
            "test-v1".to_owned(),
            &SecretString::from(
                r#"{"keys":{"test-v1":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#.to_owned(),
            ),
        )
        .expect("test credential keyring");
        let fake_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake connection server");
        let fake_port = fake_listener
            .local_addr()
            .expect("fake server address")
            .port();
        tokio::spawn(async move {
            let fake = Router::new()
                .route("/health", get(|| async { StatusCode::NO_CONTENT }))
                .route(
                    "/slow",
                    get(|| async {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        StatusCode::OK
                    }),
                );
            axum::serve(fake_listener, fake)
                .await
                .expect("serve fake connection server");
        });
        let object_store = Arc::new(InMemory::new());
        let router = super::build_api_router(AppState::new(pool.clone(), auth_settings()).with_m2(
            keyring,
            Some(object_store.clone()),
            ConnectionSettings {
                timeout_seconds: 1,
                max_concurrency: 4,
                allow_private_networks: false,
                allowed_hosts: vec!["localhost".to_owned()],
                allowed_cidrs: Vec::new(),
            },
        ));
        let bootstrap_body = json!({
            "companyName": "Agentx Test",
            "adminUsername": "admin",
            "adminDisplayName": "Company Admin",
            "password": "correct horse battery staple",
            "locale": "zh-CN",
            "timezone": "Asia/Shanghai"
        });
        let (first, second) = tokio::join!(
            router.clone().oneshot(json_request(
                "POST",
                "/api/v1/bootstrap",
                bootstrap_body.clone(),
                None,
            )),
            router.clone().oneshot(json_request(
                "POST",
                "/api/v1/bootstrap",
                bootstrap_body,
                None,
            )),
        );
        let mut responses = vec![
            first.expect("first bootstrap"),
            second.expect("second bootstrap"),
        ];
        responses.sort_by_key(|response| response.status());
        assert_eq!(responses[0].status(), StatusCode::CREATED);
        assert_eq!(responses[1].status(), StatusCode::CONFLICT);
        let original_refresh_cookie = responses[0]
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("bootstrap refresh cookie")
            .to_owned();
        let bootstrap = response_json(responses.remove(0)).await;
        let conflict_request_id = responses[0]
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .expect("error request id header")
            .to_owned();
        let conflict = response_json(responses.remove(0)).await;
        assert_eq!(conflict["requestId"], conflict_request_id);
        let access_token = bootstrap["accessToken"]
            .as_str()
            .expect("bootstrap access token")
            .to_owned();

        let refresh_request = |cookie: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/refresh")
                .header("cookie", cookie)
                .body(Body::empty())
                .expect("refresh request")
        };
        let refreshed = router
            .clone()
            .oneshot(refresh_request(&original_refresh_cookie))
            .await
            .expect("rotate refresh token");
        assert_eq!(refreshed.status(), StatusCode::OK);
        let rotated_refresh_cookie = refreshed
            .headers()
            .get("set-cookie")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("rotated refresh cookie")
            .to_owned();
        let replayed = router
            .clone()
            .oneshot(refresh_request(&original_refresh_cookie))
            .await
            .expect("replay old refresh token");
        assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(replayed).await["code"],
            "REFRESH_REPLAY_DETECTED"
        );
        let revoked_family = router
            .clone()
            .oneshot(refresh_request(&rotated_refresh_cookie))
            .await
            .expect("use revoked token family");
        assert_eq!(revoked_family.status(), StatusCode::UNAUTHORIZED);

        let first_failure = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/auth/login",
                json!({"username":"admin","password":"wrong password value"}),
                None,
            ))
            .await
            .expect("first failed login");
        assert_eq!(first_failure.status(), StatusCode::UNAUTHORIZED);
        let second_failure = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/auth/login",
                json!({"username":"admin","password":"wrong password value"}),
                None,
            ))
            .await
            .expect("second failed login");
        assert_eq!(second_failure.status(), StatusCode::TOO_MANY_REQUESTS);
        let limited = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/auth/login",
                json!({"username":"admin","password":"correct horse battery staple"}),
                None,
            ))
            .await
            .expect("rate-limited login");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let failed_audits: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action IN ('auth.login.failed','auth.login.rate_limited')",
        )
        .fetch_one(&pool)
        .await
        .expect("login failure audits");
        assert_eq!(failed_audits, 3);

        let root_department: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM departments WHERE is_root=TRUE")
                .fetch_one(&pool)
                .await
                .expect("root department");
        let member_role: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM roles WHERE code='member'")
                .fetch_one(&pool)
                .await
                .expect("member role");

        let create_department = |name: &str, parent_id: uuid::Uuid| {
            json_request(
                "POST",
                "/api/v1/departments",
                json!({"name":name,"parentId":parent_id}),
                Some(&access_token),
            )
        };
        let parent = router
            .clone()
            .oneshot(create_department("Parent", root_department))
            .await
            .expect("create parent department");
        assert_eq!(parent.status(), StatusCode::CREATED);
        let parent_id: uuid::Uuid =
            serde_json::from_value(response_json(parent).await["id"].clone())
                .expect("parent department id");
        let target = router
            .clone()
            .oneshot(create_department("Target", root_department))
            .await
            .expect("create target department");
        let target_id: uuid::Uuid =
            serde_json::from_value(response_json(target).await["id"].clone())
                .expect("target department id");
        let child = router
            .clone()
            .oneshot(create_department("Child", parent_id))
            .await
            .expect("create child department");
        let child_id: uuid::Uuid = serde_json::from_value(response_json(child).await["id"].clone())
            .expect("child department id");
        let moved = router
            .clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/v1/departments/{child_id}"),
                json!({"name":"Child","parentId":target_id,"version":1}),
                Some(&access_token),
            ))
            .await
            .expect("move child department");
        assert_eq!(moved.status(), StatusCode::OK);
        let old_path: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM department_closure WHERE ancestor_id=? AND descendant_id=?",
        )
        .bind(parent_id)
        .bind(child_id)
        .fetch_one(&pool)
        .await
        .expect("old closure path");
        let new_path: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM department_closure WHERE ancestor_id=? AND descendant_id=?",
        )
        .bind(target_id)
        .bind(child_id)
        .fetch_one(&pool)
        .await
        .expect("new closure path");
        assert_eq!((old_path, new_path), (0, 1));
        let created_user = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/users",
                json!({
                    "username":"invited.user",
                    "displayName":"Invited User",
                    "departmentId":root_department,
                    "roleId":member_role
                }),
                Some(&access_token),
            ))
            .await
            .expect("create invited user");
        assert_eq!(created_user.status(), StatusCode::CREATED);
        let created_user = response_json(created_user).await;
        assert_eq!(created_user["status"], "invited");

        let temporary_login = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/auth/login",
                json!({"username":"invited.user","password":"123456"}),
                None,
            ))
            .await
            .expect("temporary password login");
        assert_eq!(temporary_login.status(), StatusCode::OK);
        let temporary_login = response_json(temporary_login).await;
        assert_eq!(temporary_login["passwordChangeRequired"], true);
        let change_token = temporary_login["changePasswordToken"]
            .as_str()
            .expect("change password token");
        let changed = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/auth/change-password",
                json!({"token":change_token,"password":"permanent password value"}),
                None,
            ))
            .await
            .expect("change password");
        assert_eq!(changed.status(), StatusCode::OK);
        let invited_status: String =
            sqlx::query_scalar("SELECT status FROM users WHERE username_normalized='invited.user'")
                .fetch_one(&pool)
                .await
                .expect("activated invited user");
        assert_eq!(invited_status, "active");

        let permissions = router
            .clone()
            .oneshot(json_request(
                "GET",
                "/api/v1/permissions",
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("list permissions");
        assert_eq!(permissions.status(), StatusCode::OK);
        assert_eq!(
            response_json(permissions)
                .await
                .as_array()
                .expect("permission list")
                .len(),
            34
        );

        let environments = router
            .clone()
            .oneshot(json_request(
                "GET",
                "/api/v1/environments",
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("list bootstrap environments");
        assert_eq!(environments.status(), StatusCode::OK);
        let environments = response_json(environments).await;
        let environments = environments.as_array().expect("environment list");
        assert_eq!(environments.len(), 2);
        let development_id: uuid::Uuid = serde_json::from_value(
            environments
                .iter()
                .find(|item| item["code"] == "development")
                .expect("development environment")["id"]
                .clone(),
        )
        .expect("development id");
        let production_id: uuid::Uuid = serde_json::from_value(
            environments
                .iter()
                .find(|item| item["code"] == "production")
                .expect("production environment")["id"]
                .clone(),
        )
        .expect("production id");

        let credential = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/credentials",
                json!({
                    "name":"M2 OpenAI Key",
                    "credentialType":"bearer",
                    "secret":"m2-super-secret-value",
                    "ownerDepartmentId":root_department
                }),
                Some(&access_token),
            ))
            .await
            .expect("create credential");
        assert_eq!(credential.status(), StatusCode::CREATED);
        let credential = response_json(credential).await;
        assert!(!credential.to_string().contains("m2-super-secret-value"));
        let credential_id: uuid::Uuid =
            serde_json::from_value(credential["id"].clone()).expect("credential id");
        let renamed_credential = router
            .clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/v1/credentials/{credential_id}"),
                json!({"name":"M2 Renamed Key","status":"active","version":1}),
                Some(&access_token),
            ))
            .await
            .expect("rename credential");
        assert_eq!(renamed_credential.status(), StatusCode::OK);
        let renamed_credential = response_json(renamed_credential).await;
        assert_eq!(renamed_credential["currentSecretVersion"], 1);
        let credential_version = renamed_credential["version"]
            .as_u64()
            .expect("renamed credential version");
        let secret_versions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM credential_secret_versions WHERE credential_id=?",
        )
        .bind(credential_id)
        .fetch_one(&pool)
        .await
        .expect("credential secret versions");
        assert_eq!(secret_versions, 1, "renaming must not rotate the secret");

        let provider = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/models/providers",
                json!({
                    "name":"M2 Provider",
                    "providerType":"openai_compatible",
                    "endpoint":"https://models.example.test/v1",
                    "credentialId":credential_id,
                    "ownerDepartmentId":root_department
                }),
                Some(&access_token),
            ))
            .await
            .expect("create provider");
        assert_eq!(provider.status(), StatusCode::CREATED);
        let provider_id: uuid::Uuid =
            serde_json::from_value(response_json(provider).await["id"].clone())
                .expect("provider id");
        let deployment = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/models/deployments",
                json!({
                    "providerId":provider_id,
                    "name":"M2 GPT Deployment",
                    "modelName":"gpt-m2",
                    "endpointOverride":null,
                    "credentialId":null,
                    "defaultParameters":{}
                }),
                Some(&access_token),
            ))
            .await
            .expect("create model deployment");
        assert_eq!(deployment.status(), StatusCode::CREATED);
        let deployment_id: uuid::Uuid =
            serde_json::from_value(response_json(deployment).await["id"].clone())
                .expect("deployment id");
        let model = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/models/aliases",
                json!({"alias":"m2-chat","deploymentId":deployment_id}),
                Some(&access_token),
            ))
            .await
            .expect("create model alias");
        assert_eq!(model.status(), StatusCode::CREATED);
        let model_id: uuid::Uuid = serde_json::from_value(response_json(model).await["id"].clone())
            .expect("model alias id");
        let revised_model = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/models/aliases/{model_id}/deployment-revisions"),
                json!({
                    "providerId":provider_id,
                    "name":"M2 GPT Deployment",
                    "modelName":"gpt-m2-r2",
                    "endpointOverride":"https://models.example.test/v2",
                    "credentialId":credential_id,
                    "defaultParameters":{"temperature":0.1},
                    "expectedAliasVersion":1,
                    "price":{"currency":"USD","inputPerMillion":"1.25","outputPerMillion":"2.50"}
                }),
                Some(&access_token),
            ))
            .await
            .expect("create model deployment revision");
        assert_eq!(revised_model.status(), StatusCode::CREATED);
        let revised_model = response_json(revised_model).await;
        assert_eq!(revised_model["modelName"], "gpt-m2-r2");
        assert_eq!(revised_model["aliasVersion"], 2);
        let history = router
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/v1/models/aliases/{model_id}/deployment-history"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("load deployment history");
        assert_eq!(history.status(), StatusCode::OK);
        let history = response_json(history).await;
        assert_eq!(history.as_array().expect("deployment history").len(), 2);
        assert!(
            history[0]["changedAt"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z')),
            "timestamps use RFC3339"
        );
        let original_model_name: String =
            sqlx::query_scalar("SELECT model_name FROM model_deployments WHERE id=?")
                .bind(deployment_id)
                .fetch_one(&pool)
                .await
                .expect("load original deployment");
        assert_eq!(original_model_name, "gpt-m2");

        let workflow = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/workflows",
                json!({"name":"M2 Workflow","description":"Control plane closure","visibility":"private"}),
                Some(&access_token),
            ))
            .await
            .expect("create workflow");
        assert_eq!(workflow.status(), StatusCode::CREATED);
        let workflow = response_json(workflow).await;
        let workflow_id: uuid::Uuid =
            serde_json::from_value(workflow["id"].clone()).expect("workflow id");
        let service_identity_id: uuid::Uuid =
            serde_json::from_value(workflow["serviceIdentityId"].clone())
                .expect("workflow service identity id");
        let definition = json!({
            "schemaVersion":"1.0",
            "nodes":[
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Manual Trigger","position":{"x":100,"y":160},"disabled":false,"parameters":{},"resourceReferences":[]},
                {"id":"model","type":"model","typeVersion":1,"name":"Model","position":{"x":420,"y":160},"disabled":false,"parameters":{},"resourceReferences":[{"resourceType":"model","resourceId":model_id,"resourceVersionId":null,"operation":"use"}]}
            ],
            "connections":[{"id":"trigger-model","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"model","targetHandle":"main"}],
            "settings":{}
        });
        let saved = router
            .clone()
            .oneshot(idempotent_json_request(
                "PUT",
                &format!("/api/v1/workflows/{workflow_id}/draft"),
                json!({"expectedRevision":0,"definition":definition}),
                &access_token,
                "m2-draft-1",
            ))
            .await
            .expect("save M2 draft");
        assert_eq!(saved.status(), StatusCode::OK);
        assert_eq!(response_json(saved).await["revision"], 1);
        let stale = router
            .clone()
            .oneshot(json_request(
                "PUT",
                &format!("/api/v1/workflows/{workflow_id}/draft"),
                json!({"expectedRevision":0,"definition":definition}),
                Some(&access_token),
            ))
            .await
            .expect("reject stale draft");
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(stale).await["code"],
            "DRAFT_REVISION_CONFLICT"
        );

        let validation = router
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/v1/workflows/{workflow_id}/resource-validation"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("validate missing grants");
        let validation = response_json(validation).await;
        assert_eq!(validation["valid"], false);
        let missing = validation["missingGrants"]
            .as_array()
            .expect("missing grants");
        assert!(missing.iter().any(|item| item["resourceType"] == "model"));
        assert!(
            missing
                .iter()
                .any(|item| item["resourceType"] == "credential")
        );

        let model_grant = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/resources/model/{model_id}/grants"),
                json!({"subjectType":"workflow_service_identity","subjectId":service_identity_id,"resourceVersionId":null,"operation":"use"}),
                &access_token,
                "m2-model-grant",
            ))
            .await
            .expect("grant model");
        assert_eq!(model_grant.status(), StatusCode::CREATED);
        let credential_grant = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/resources/credential/{credential_id}/grants"),
                json!({"subjectType":"workflow_service_identity","subjectId":service_identity_id,"resourceVersionId":null,"operation":"use"}),
                &access_token,
                "m2-credential-grant",
            ))
            .await
            .expect("grant credential");
        assert_eq!(credential_grant.status(), StatusCode::CREATED);
        let credential_grant_id: uuid::Uuid =
            serde_json::from_value(response_json(credential_grant).await["id"].clone())
                .expect("credential grant id");

        let version_request = json!({"draftRevision":1});
        let version = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/versions"),
                version_request.clone(),
                &access_token,
                "m2-version-1",
            ))
            .await
            .expect("create workflow version");
        let version_status = version.status();
        let version = response_json(version).await;
        assert_eq!(version_status, StatusCode::CREATED, "{version}");
        let version_id: uuid::Uuid =
            serde_json::from_value(version["id"].clone()).expect("workflow version id");
        let replayed_version = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/versions"),
                version_request,
                &access_token,
                "m2-version-1",
            ))
            .await
            .expect("replay workflow version");
        assert_eq!(replayed_version.status(), StatusCode::OK);
        assert_eq!(response_json(replayed_version).await["id"], version["id"]);
        let reused_key = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/versions"),
                json!({"draftRevision":2}),
                &access_token,
                "m2-version-1",
            ))
            .await
            .expect("reject reused idempotency key");
        assert_eq!(reused_key.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(reused_key).await["code"],
            "IDEMPOTENCY_KEY_REUSED"
        );

        let published = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments"),
                json!({"environmentId":development_id,"workflowVersionId":version_id}),
                &access_token,
                "m2-publish-development",
            ))
            .await
            .expect("publish workflow");
        assert_eq!(published.status(), StatusCode::CREATED);
        let revoked = router
            .clone()
            .oneshot(json_request(
                "DELETE",
                &format!(
                    "/api/v1/resources/credential/{credential_id}/grants/{credential_grant_id}"
                ),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("revoke credential grant");
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
        let blocked_publish = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments"),
                json!({"environmentId":production_id,"workflowVersionId":version_id}),
                &access_token,
                "m2-publish-production-blocked",
            ))
            .await
            .expect("block publish after revocation");
        assert_eq!(blocked_publish.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response_json(blocked_publish).await["code"],
            "RESOURCE_GRANT_MISSING"
        );
        let restored_grant = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/resources/credential/{credential_id}/grants"),
                json!({"subjectType":"workflow_service_identity","subjectId":service_identity_id,"resourceVersionId":null,"operation":"use"}),
                &access_token,
                "m2-credential-grant-restored",
            ))
            .await
            .expect("restore credential grant");
        assert_eq!(restored_grant.status(), StatusCode::CREATED);
        let disabled_credential = router
            .clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/v1/credentials/{credential_id}"),
                json!({"name":"M2 OpenAI Key","status":"disabled","version":credential_version}),
                Some(&access_token),
            ))
            .await
            .expect("disable credential");
        assert_eq!(disabled_credential.status(), StatusCode::OK);
        let disabled_credential = response_json(disabled_credential).await;
        let disabled_publish = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments"),
                json!({"environmentId":production_id,"workflowVersionId":version_id}),
                &access_token,
                "m2-publish-disabled-resource",
            ))
            .await
            .expect("block disabled resource publish");
        assert_eq!(disabled_publish.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response_json(disabled_publish).await["code"],
            "RESOURCE_UNAVAILABLE"
        );
        let enabled_credential = router
            .clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/v1/credentials/{credential_id}"),
                json!({"name":"M2 OpenAI Key","status":"active","version":disabled_credential["version"]}),
                Some(&access_token),
            ))
            .await
            .expect("enable credential");
        assert_eq!(enabled_credential.status(), StatusCode::OK);
        let (concurrent_a, concurrent_b) = tokio::join!(
            router.clone().oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments"),
                json!({"environmentId":production_id,"workflowVersionId":version_id}),
                &access_token,
                "m2-concurrent-publish-a",
            )),
            router.clone().oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments"),
                json!({"environmentId":production_id,"workflowVersionId":version_id}),
                &access_token,
                "m2-concurrent-publish-b",
            )),
        );
        assert_eq!(
            concurrent_a.expect("first concurrent publish").status(),
            StatusCode::CREATED
        );
        assert_eq!(
            concurrent_b.expect("second concurrent publish").status(),
            StatusCode::CREATED
        );
        let active_production: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_deployments WHERE tenant_id=(SELECT tenant_id FROM workflows WHERE id=?) AND workflow_id=? AND environment_id=? AND status='active'")
            .bind(workflow_id).bind(workflow_id).bind(production_id).fetch_one(&pool).await.expect("active production deployment");
        assert_eq!(active_production, 1);
        let rollback = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/deployments/{development_id}/rollback"),
                json!({"targetWorkflowVersionId":version_id}),
                &access_token,
                "m2-rollback-development",
            ))
            .await
            .expect("rollback workflow");
        assert_eq!(rollback.status(), StatusCode::CREATED);
        assert_eq!(response_json(rollback).await["source"], "rollback");
        let runtime = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/workflows/{workflow_id}/run"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("runtime remains unavailable");
        assert_eq!(runtime.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_json(runtime).await["code"], "RUNTIME_UNAVAILABLE");
        let snapshot_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_version_resources WHERE workflow_version_id=?",
        )
        .bind(version_id)
        .fetch_one(&pool)
        .await
        .expect("version snapshots");
        assert_eq!(snapshot_count, 2);
        let fake_runtime_tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('executions','trace_events','evaluation_runs')")
            .fetch_one(&pool).await.expect("runtime table check");
        assert_eq!(fake_runtime_tables, 0);

        let rag_connection = router.clone().oneshot(json_request("POST","/api/v1/knowledge/connections",json!({"name":"M2 LightRAG","endpoint":"http://127.0.0.1:9","healthPath":"/health","credentialId":credential_id,"ownerDepartmentId":root_department,"configuration":{}}),Some(&access_token))).await.expect("create RAG connection");
        assert_eq!(rag_connection.status(), StatusCode::CREATED);
        let rag_connection_id: uuid::Uuid =
            serde_json::from_value(response_json(rag_connection).await["id"].clone()).unwrap();
        let ssrf = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/knowledge/connections/{rag_connection_id}/test-connection"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("block local connection test");
        assert_eq!(ssrf.status(), StatusCode::FORBIDDEN);
        let healthy_connection = router.clone().oneshot(json_request("POST","/api/v1/knowledge/connections",json!({"name":"M2 Healthy LightRAG","endpoint":format!("http://localhost:{fake_port}"),"healthPath":"/health","credentialId":null,"ownerDepartmentId":root_department,"configuration":{}}),Some(&access_token))).await.expect("create healthy RAG connection");
        assert_eq!(healthy_connection.status(), StatusCode::CREATED);
        let healthy_connection_id: uuid::Uuid =
            serde_json::from_value(response_json(healthy_connection).await["id"].clone())
                .expect("healthy connection id");
        let healthy_check = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/knowledge/connections/{healthy_connection_id}/test-connection"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("test healthy RAG connection");
        assert_eq!(healthy_check.status(), StatusCode::OK);
        assert_eq!(response_json(healthy_check).await["status"], "healthy");
        let slow_connection = router.clone().oneshot(json_request("POST","/api/v1/memory/connections",json!({"name":"M2 Slow Mem0","endpoint":format!("http://localhost:{fake_port}"),"healthPath":"/slow","credentialId":null,"ownerDepartmentId":root_department,"configuration":{}}),Some(&access_token))).await.expect("create slow memory connection");
        let slow_connection_id: uuid::Uuid =
            serde_json::from_value(response_json(slow_connection).await["id"].clone())
                .expect("slow connection id");
        let slow_check = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/memory/connections/{slow_connection_id}/test-connection"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("test connection timeout");
        assert_eq!(slow_check.status(), StatusCode::OK);
        let slow_check = response_json(slow_check).await;
        assert_eq!(slow_check["status"], "unhealthy");
        assert_eq!(slow_check["errorCode"], "CONNECTION_TIMEOUT");
        assert_eq!(slow_check["errorMessage"], "Connection test timed out");
        let knowledge = router.clone().oneshot(json_request("POST","/api/v1/knowledge/resources",json!({"connectionId":rag_connection_id,"name":"M2 Knowledge","externalResourceId":"kb-m2","ownerDepartmentId":root_department}),Some(&access_token))).await.expect("create knowledge resource");
        assert_eq!(knowledge.status(), StatusCode::CREATED);
        let memory_connection = router.clone().oneshot(json_request("POST","/api/v1/memory/connections",json!({"name":"M2 Mem0","endpoint":"https://memory.example.test","healthPath":"/health","credentialId":credential_id,"ownerDepartmentId":root_department,"configuration":{}}),Some(&access_token))).await.expect("create memory connection");
        let memory_connection_id: uuid::Uuid =
            serde_json::from_value(response_json(memory_connection).await["id"].clone()).unwrap();
        let memory = router.clone().oneshot(json_request("POST","/api/v1/memory/namespaces",json!({"connectionId":memory_connection_id,"name":"M2 Memory","externalNamespace":"namespace-m2","accessMode":"read_write","ownerDepartmentId":root_department}),Some(&access_token))).await.expect("create memory namespace");
        assert_eq!(memory.status(), StatusCode::CREATED);

        let skill = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/skills",
                json!({"name":"Workspace Integration Skill","description":"test","ownerDepartmentId":root_department}),
                Some(&access_token),
            ))
            .await
            .expect("create workspace skill");
        assert_eq!(skill.status(), StatusCode::CREATED);
        let skill_id: uuid::Uuid = serde_json::from_value(response_json(skill).await["id"].clone())
            .expect("workspace skill id");
        let workspace = router
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/v1/skills/{skill_id}/workspace"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("load initial skill workspace");
        let workspace = response_json(workspace).await;
        assert_eq!(workspace["revision"], 1);
        assert_eq!(workspace["entries"][0]["path"], "SKILL.md");
        let directory = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/skills/{skill_id}/entries"),
                json!({"parentId":null,"name":"docs","entryType":"directory","expectedRevision":1}),
                Some(&access_token),
            ))
            .await
            .expect("create skill directory");
        let directory = response_json(directory).await;
        let directory_id: uuid::Uuid = serde_json::from_value(
            directory["entries"]
                .as_array()
                .and_then(|entries| entries.iter().find(|entry| entry["path"] == "docs"))
                .expect("skill directory entry")["id"]
                .clone(),
        )
        .expect("skill directory id");
        let markdown = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/skills/{skill_id}/entries"),
                json!({"parentId":directory_id,"name":"guide.md","entryType":"file","expectedRevision":2}),
                Some(&access_token),
            ))
            .await
            .expect("create skill markdown");
        let markdown = response_json(markdown).await;
        let markdown_id: uuid::Uuid = serde_json::from_value(
            markdown["entries"]
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry["path"] == "docs/guide.md")
                })
                .expect("skill markdown entry")["id"]
                .clone(),
        )
        .expect("skill markdown id");
        let saved_markdown = router
            .clone()
            .oneshot(json_request(
                "PUT",
                &format!("/api/v1/skills/{skill_id}/files/{markdown_id}"),
                json!({"content":"# Guide\n[Root](../SKILL.md)\n","expectedRevision":3}),
                Some(&access_token),
            ))
            .await
            .expect("save skill markdown");
        assert_eq!(saved_markdown.status(), StatusCode::OK);
        let skill_version = router
            .clone()
            .oneshot(idempotent_json_request(
                "POST",
                &format!("/api/v1/skills/{skill_id}/versions"),
                json!({"expectedRevision":4,"dependencies":[]}),
                &access_token,
                "m2-skill-version-1",
            ))
            .await
            .expect("publish skill workspace");
        assert_eq!(skill_version.status(), StatusCode::CREATED);
        let skill_version = response_json(skill_version).await;
        assert_eq!(skill_version["fileCount"], 2);
        let frozen_files: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM skill_version_files WHERE skill_version_id=?")
                .bind(
                    serde_json::from_value::<uuid::Uuid>(skill_version["id"].clone())
                        .expect("skill version id"),
                )
                .fetch_one(&pool)
                .await
                .expect("frozen skill files");
        assert_eq!(frozen_files, 2);

        let tenant_id = TenantId::new();
        sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Artifact Test','artifact test')")
            .bind(tenant_id.as_uuid()).execute(&pool).await.expect("test tenant");
        let foreign_department = uuid::Uuid::now_v7();
        let foreign_user = uuid::Uuid::now_v7();
        sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Foreign','foreign',TRUE)")
            .bind(foreign_department).bind(tenant_id.as_uuid()).execute(&pool).await.expect("foreign department");
        sqlx::query("INSERT INTO department_closure(tenant_id,ancestor_id,descendant_id,depth) VALUES(?,?,?,0)")
            .bind(tenant_id.as_uuid()).bind(foreign_department).bind(foreign_department).execute(&pool).await.expect("foreign closure");
        sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status) VALUES(?,?,'foreign.user','foreign.user','Foreign User','active')")
            .bind(foreign_user).bind(tenant_id.as_uuid()).execute(&pool).await.expect("foreign user");
        sqlx::query("INSERT INTO user_departments(tenant_id,user_id,department_id) VALUES(?,?,?)")
            .bind(tenant_id.as_uuid())
            .bind(foreign_user)
            .bind(foreign_department)
            .execute(&pool)
            .await
            .expect("foreign user department");
        let users = router
            .clone()
            .oneshot(json_request(
                "GET",
                "/api/v1/users?pageSize=100",
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("tenant-scoped users");
        let users = response_json(users).await.to_string();
        assert!(!users.contains("foreign.user"));
        let foreign_credential = uuid::Uuid::now_v7();
        sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,'Foreign Secret','bearer','****test',?,?)")
            .bind(foreign_credential).bind(tenant_id.as_uuid()).bind(foreign_department).bind(foreign_user).execute(&pool).await.expect("foreign credential");
        let foreign_read = router
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/v1/credentials/{foreign_credential}"),
                json!({}),
                Some(&access_token),
            ))
            .await
            .expect("reject cross-tenant credential read");
        assert_eq!(foreign_read.status(), StatusCode::NOT_FOUND);
        let artifacts = agentx_infrastructure::artifact::MySqlObjectArtifactStore::new(
            pool.clone(),
            std::sync::Arc::new(InMemory::new()),
        );
        let written = artifacts
            .put(ArtifactWrite {
                tenant_id,
                content_type: "application/json".to_owned(),
                content: br#"{"ok":true}"#.to_vec(),
            })
            .await
            .expect("write artifact");
        let loaded = artifacts
            .get(tenant_id, written.id)
            .await
            .expect("read artifact")
            .expect("artifact exists");
        assert_eq!(loaded.content, written.content);
        assert_eq!(loaded.sha256, written.sha256);

        let mut transaction = pool.begin().await.expect("begin outbox transaction");
        sqlx::query("INSERT INTO outbox_events(id,tenant_id,event_type,aggregate_type,aggregate_id,payload_json) VALUES(?,?, 'Tested','tenant',?,JSON_OBJECT())")
            .bind(uuid::Uuid::now_v7()).bind(tenant_id.as_uuid()).bind(tenant_id.to_string())
            .execute(&mut *transaction).await.expect("append outbox event");
        transaction
            .rollback()
            .await
            .expect("rollback outbox transaction");
        let outbox_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE tenant_id=?")
                .bind(tenant_id.as_uuid())
                .fetch_one(&pool)
                .await
                .expect("outbox count");
        assert_eq!(outbox_count, 0);

        let outbox = agentx_infrastructure::outbox::MySqlOutbox::new(pool.clone());
        let message = OutboxMessage::new(
            tenant_id,
            "M1Tested",
            "tenant",
            tenant_id.to_string(),
            json!({"ok":true}),
        );
        outbox
            .append(message.clone())
            .await
            .expect("append outbox event");
        assert!(
            outbox.append(message).await.is_err(),
            "event id is idempotent"
        );
        let dispatcher = agentx_infrastructure::outbox::MySqlOutboxDispatcher::new(pool.clone());
        let first_delivery = dispatcher
            .claim(10, Duration::from_secs(30))
            .await
            .expect("claim outbox event")
            .pop()
            .expect("leased event");
        assert_eq!(first_delivery.attempt_count, 1);
        assert!(
            dispatcher
                .mark_failed(&first_delivery, "temporary failure", Duration::from_secs(1))
                .await
                .expect("reschedule event")
        );
        sqlx::query("UPDATE outbox_events SET available_at=CURRENT_TIMESTAMP(6) WHERE id=?")
            .bind(first_delivery.event_id)
            .execute(&pool)
            .await
            .expect("make retry available");
        let second_delivery = dispatcher
            .claim(10, Duration::from_secs(30))
            .await
            .expect("reclaim outbox event")
            .pop()
            .expect("retried event");
        assert_eq!(second_delivery.attempt_count, 2);
        assert!(
            !dispatcher
                .mark_published(&first_delivery)
                .await
                .expect("stale lease is rejected")
        );
        assert!(
            dispatcher
                .mark_published(&second_delivery)
                .await
                .expect("publish event")
        );
    }

    #[tokio::test]
    async fn optional_infrastructure_containers_are_self_contained() {
        let redis = GenericImage::new("redis", "7.4-alpine")
            .with_exposed_port(6379.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .await
            .expect("Redis container");
        let clickhouse = GenericImage::new("clickhouse/clickhouse-server", "25.3")
            .with_exposed_port(8123.tcp())
            .with_wait_for(WaitFor::seconds(4))
            .with_env_var("CLICKHOUSE_DB", "agentx")
            .with_env_var("CLICKHOUSE_USER", "agentx")
            .with_env_var("CLICKHOUSE_PASSWORD", "agentx-test-password")
            .with_env_var("CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT", "1")
            .start()
            .await
            .expect("ClickHouse container");
        let minio = GenericImage::new("quay.io/minio/minio", "latest")
            .with_exposed_port(9000.tcp())
            .with_wait_for(WaitFor::seconds(3))
            .with_env_var("MINIO_ROOT_USER", "agentx")
            .with_env_var("MINIO_ROOT_PASSWORD", "agentx-test-secret")
            .with_cmd(["server", "/data"])
            .start()
            .await
            .expect("MinIO container");

        let redis_port = redis
            .get_host_port_ipv4(6379.tcp())
            .await
            .expect("Redis port");
        let mut redis_ready = false;
        for _ in 0..20 {
            if let Ok(mut connection) = clients::connect_redis(&RedisSettings {
                url: SecretString::from(format!("redis://127.0.0.1:{redis_port}/")),
            })
            .await
            {
                if redis::cmd("PING")
                    .query_async::<String>(&mut connection)
                    .await
                    .is_ok()
                {
                    redis_ready = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(redis_ready);

        let clickhouse_port = clickhouse
            .get_host_port_ipv4(8123.tcp())
            .await
            .expect("ClickHouse port");
        let clickhouse_client = clients::clickhouse(&ClickHouseSettings {
            url: format!("http://127.0.0.1:{clickhouse_port}"),
            database: "agentx".to_owned(),
            username: "agentx".to_owned(),
            password: SecretString::from("agentx-test-password".to_owned()),
        });
        let mut clickhouse_ready = false;
        for _ in 0..20 {
            if clickhouse_client
                .query("SELECT 1")
                .fetch_one::<u8>()
                .await
                .is_ok()
            {
                clickhouse_ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(clickhouse_ready);

        let minio_port = minio
            .get_host_port_ipv4(9000.tcp())
            .await
            .expect("MinIO port");
        let mut minio_ready = false;
        for _ in 0..20 {
            if tokio::net::TcpStream::connect(("127.0.0.1", minio_port))
                .await
                .is_ok()
            {
                minio_ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(minio_ready);
    }

    #[tokio::test]
    async fn mysql_migrations_upgrade_an_m1_schema_to_mcp_and_skill_workspace() {
        let container = GenericImage::new("mysql", "8.4")
            .with_exposed_port(3306.tcp())
            .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
            .with_env_var("MYSQL_DATABASE", "agentx")
            .with_env_var("MYSQL_USER", "agentx")
            .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
            .start()
            .await
            .expect("MySQL container should start");
        let port = container
            .get_host_port_ipv4(3306.tcp())
            .await
            .expect("mapped MySQL port");
        let settings = MySqlSettings {
            host: "127.0.0.1".to_owned(),
            port,
            database: "agentx".to_owned(),
            username: "agentx".to_owned(),
            password: SecretString::from("agentx-test-password".to_owned()),
            max_connections: 5,
        };
        let mut pool = None;
        for _ in 0..30 {
            match mysql::connect(&settings).await {
                Ok(value) => {
                    pool = Some(value);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
        let pool = pool.expect("connect to test MySQL");
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/mysql"
        ));
        let all = sqlx::migrate::Migrator::new(path)
            .await
            .expect("load migrations");
        let m1 = sqlx::migrate::Migrator {
            migrations: Cow::Owned(
                all.migrations
                    .iter()
                    .filter(|migration| migration.version <= 3)
                    .cloned()
                    .collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        m1.run(&pool).await.expect("apply M1 migrations");
        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='mcp_servers'",
        )
        .fetch_one(&pool)
        .await
        .expect("query pre-upgrade schema");
        assert_eq!(before, 0);

        all.run(&pool).await.expect("append M2.1 migrations");
        let current_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('mcp_servers','mcp_tools','skill_workspace_entries')",
        )
        .fetch_one(&pool)
        .await
        .expect("query M2.1 schema");
        assert_eq!(current_tables, 3);
        let removed_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name IN ('tools','tool_versions')",
        )
        .fetch_one(&pool)
        .await
        .expect("query removed schema");
        assert_eq!(removed_tables, 0);
    }
}
