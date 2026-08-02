mod auth;
mod config;
mod error;
mod iam;
mod models;
mod security;
mod state;

use std::{env, time::Duration};

use agentx_infrastructure::{config::InfrastructureSettings, mysql};
use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, patch, post},
};
use config::AuthSettings;
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
        iam::list_permissions
    ),
    components(schemas(
        BootstrapStatus, BootstrapRequest, LoginRequest, ChangePasswordRequest, AuthResponse,
        MeResponse, DepartmentResponse, CreateDepartmentRequest, UpdateDepartmentRequest,
        UserResponse, CreateUserRequest, UpdateUserRequest, RoleResponse, CreateRoleRequest,
        UpdateRoleRequest, PermissionResponse, agentx_api_types::ApiErrorResponse,
        agentx_api_types::FieldError,
        agentx_api_types::HealthResponse, agentx_api_types::DependencyHealth
    )),
    tags((name = "Agentx M1", description = "Bootstrap, authentication and IAM control plane"))
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
    let state = AppState::new(pool.clone(), auth);
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
    Router::new().nest("/api/v1", api).with_state(state)
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
                "SELECT COALESCE(MAX(version),0) >= 3 AND COALESCE(MIN(success),0)=1 FROM _sqlx_migrations",
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
        mysql,
    };
    use object_store::memory::InMemory;
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use std::time::Duration;
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use tower::ServiceExt;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::Response,
    };

    use crate::{config::AuthSettings, state::AppState};

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

        let router = super::build_api_router(AppState::new(pool.clone(), auth_settings()));
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
            12
        );

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
}
