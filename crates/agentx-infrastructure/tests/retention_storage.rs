use std::sync::Arc;

use agentx_infrastructure::{
    clients,
    config::{ClickHouseSettings, MySqlSettings, MySqlTlsMode},
    mysql,
    retention::RetentionProcessor,
};
use bytes::Bytes;
use object_store::{ObjectStore, memory::InMemory, path::Path};
use secrecy::SecretString;
use sqlx::{MySqlPool, Row};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

#[tokio::test]
async fn retention_blocks_references_and_recovers_after_object_store_outage() {
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
        host: "127.0.0.1".into(),
        port,
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("agentx-test-password".to_owned()),
        max_connections: 5,
        tls_mode: MySqlTlsMode::Disabled,
        tls_ca_path: None,
        tls_client_cert_path: None,
        tls_client_key_path: None,
    };
    let pool = connect_with_retry(&settings).await;
    mysql::run_migrations(&pool).await.expect("run migrations");

    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let run = Uuid::now_v7();
    seed_actor(&pool, tenant, user).await;
    sqlx::query("INSERT INTO retention_policies(tenant_id,data_type,retention_days,updated_by) VALUES(?,'artifact',1,?)")
        .bind(tenant)
        .bind(user)
        .execute(&pool)
        .await
        .expect("artifact retention policy");
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,requested_by) VALUES(?,?,FALSE,?)",
    )
    .bind(run)
    .bind(tenant)
    .bind(user)
    .execute(&pool)
    .await
    .expect("retention run");

    let referenced = Uuid::now_v7();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    for artifact in [referenced, first, second] {
        sqlx::query("INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key,created_at) VALUES(?,?,'text/plain',4,REPEAT('0',64),?,DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY))")
            .bind(artifact)
            .bind(tenant)
            .bind(format!("{tenant}/{artifact}"))
            .execute(&pool)
            .await
            .expect("old artifact");
    }
    sqlx::query("INSERT INTO artifact_references(tenant_id,artifact_id,owner_type,owner_id,reference_role) VALUES(?,?,'workflow_version','version-1','definition')")
        .bind(tenant)
        .bind(referenced)
        .execute(&pool)
        .await
        .expect("artifact reference");

    let unavailable = RetentionProcessor::new(pool.clone(), None, None);
    assert_eq!(unavailable.process_batch().await.expect("first batch"), 0);
    let states = item_states(&pool, run).await;
    assert_eq!(states.get("blocked").copied(), Some(1));
    assert_eq!(states.get("failed").copied(), Some(2));
    let queued: String = sqlx::query_scalar("SELECT status FROM retention_runs WHERE id=?")
        .bind(run)
        .fetch_one(&pool)
        .await
        .expect("retry status");
    assert_eq!(queued, "queued");

    let objects = Arc::new(InMemory::new());
    for artifact in [first, second] {
        objects
            .put(
                &Path::from(format!("{tenant}/{artifact}")),
                Bytes::from_static(b"data").into(),
            )
            .await
            .expect("restore artifact object");
    }
    sqlx::query("UPDATE retention_runs SET available_at=CURRENT_TIMESTAMP(6) WHERE id=?")
        .bind(run)
        .execute(&pool)
        .await
        .expect("make retry available");
    let recovered = RetentionProcessor::new(pool.clone(), Some(objects.clone()), None);
    assert_eq!(recovered.process_batch().await.expect("recovery batch"), 2);

    let row = sqlx::query("SELECT status,deleted_count FROM retention_runs WHERE id=?")
        .bind(run)
        .fetch_one(&pool)
        .await
        .expect("completed run");
    assert_eq!(row.try_get::<String, _>("status").unwrap(), "completed");
    assert_eq!(row.try_get::<u64, _>("deleted_count").unwrap(), 2);
    let deleted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM artifacts WHERE tenant_id=? AND id IN (?,?) AND deleted_at IS NOT NULL",
    )
    .bind(tenant)
    .bind(first)
    .bind(second)
    .fetch_one(&pool)
    .await
    .expect("deleted artifacts");
    assert_eq!(deleted, 2);
    let protected: bool =
        sqlx::query_scalar("SELECT deleted_at IS NULL FROM artifacts WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(referenced)
            .fetch_one(&pool)
            .await
            .expect("protected artifact");
    assert!(protected);

    verify_large_batch_progress(&pool, objects).await;
    verify_trace_failure_recovery(&pool).await;
    verify_message_and_evaluation_cleanup(&pool).await;
}

async fn verify_large_batch_progress(pool: &MySqlPool, objects: Arc<InMemory>) {
    const ARTIFACT_COUNT: usize = 501;
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let run = Uuid::now_v7();
    seed_actor(pool, tenant, user).await;
    sqlx::query("INSERT INTO retention_policies(tenant_id,data_type,retention_days,updated_by) VALUES(?,'artifact',1,?)")
        .bind(tenant)
        .bind(user)
        .execute(pool)
        .await
        .expect("large artifact retention policy");
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,requested_by) VALUES(?,?,FALSE,?)",
    )
    .bind(run)
    .bind(tenant)
    .bind(user)
    .execute(pool)
    .await
    .expect("large retention run");

    let mut tx = pool.begin().await.expect("large artifact transaction");
    for index in 0..ARTIFACT_COUNT {
        let artifact = Uuid::now_v7();
        let storage_key = format!("{tenant}/large/{artifact}");
        sqlx::query("INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key,created_at) VALUES(?,?,'text/plain',4,REPEAT('0',64),?,DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY))")
            .bind(artifact)
            .bind(tenant)
            .bind(&storage_key)
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|error| panic!("large artifact {index}: {error}"));
        objects
            .put(&Path::from(storage_key), Bytes::from_static(b"data").into())
            .await
            .unwrap_or_else(|error| panic!("large artifact object {index}: {error}"));
    }
    tx.commit().await.expect("commit large artifacts");

    let processor = RetentionProcessor::new(pool.clone(), Some(objects), None);
    let mut processed = 0_u64;
    for _ in 0..10 {
        processed += processor.process_batch().await.expect("large batch");
        let status: String = sqlx::query_scalar("SELECT status FROM retention_runs WHERE id=?")
            .bind(run)
            .fetch_one(pool)
            .await
            .expect("large run status");
        if status == "completed" {
            break;
        }
    }
    assert_eq!(processed, ARTIFACT_COUNT as u64);
    let row =
        sqlx::query("SELECT status,attempt_count,deleted_count FROM retention_runs WHERE id=?")
            .bind(run)
            .fetch_one(pool)
            .await
            .expect("completed large run");
    assert_eq!(row.try_get::<String, _>("status").unwrap(), "completed");
    assert_eq!(row.try_get::<u32, _>("attempt_count").unwrap(), 1);
    assert_eq!(
        row.try_get::<u64, _>("deleted_count").unwrap(),
        ARTIFACT_COUNT as u64
    );
}

async fn verify_trace_failure_recovery(pool: &MySqlPool) {
    let clickhouse = GenericImage::new("clickhouse/clickhouse-server", "25.3")
        .with_exposed_port(8123.tcp())
        .with_wait_for(WaitFor::seconds(4))
        .with_env_var("CLICKHOUSE_DB", "agentx")
        .with_env_var("CLICKHOUSE_USER", "agentx")
        .with_env_var("CLICKHOUSE_PASSWORD", "agentx-test-password")
        .with_env_var("CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT", "1")
        .start()
        .await
        .expect("ClickHouse container should start");
    let clickhouse_port = clickhouse
        .get_host_port_ipv4(8123.tcp())
        .await
        .expect("mapped ClickHouse port");
    let client = clients::clickhouse(&ClickHouseSettings {
        url: format!("http://127.0.0.1:{clickhouse_port}"),
        database: "agentx".into(),
        username: "agentx".into(),
        password: SecretString::from("agentx-test-password".to_owned()),
        tls_ca_path: None,
    })
    .expect("ClickHouse client");
    for _ in 0..30 {
        if client.query("SELECT 1").fetch_one::<u8>().await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    client
        .query("CREATE TABLE workflow_trace_events (tenant_id UUID, execution_id UUID) ENGINE=MergeTree ORDER BY (tenant_id,execution_id)")
        .execute()
        .await
        .expect("create trace table");

    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let department = Uuid::now_v7();
    let workflow = Uuid::now_v7();
    let version = Uuid::now_v7();
    let unprotected_execution = Uuid::now_v7();
    let protected_execution = Uuid::now_v7();
    let run = Uuid::now_v7();
    seed_actor(pool, tenant, user).await;
    sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
        .bind(department)
        .bind(tenant)
        .execute(pool)
        .await
        .expect("trace department");
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Trace Retention',?,?)")
        .bind(workflow)
        .bind(tenant)
        .bind(user)
        .bind(department)
        .execute(pool)
        .await
        .expect("trace workflow");
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'4.0',JSON_OBJECT('schemaVersion','4.0','start',JSON_OBJECT('inputs',JSON_OBJECT(),'contexts',JSON_OBJECT()),'nodes',JSON_ARRAY(),'connections',JSON_ARRAY(),'end',JSON_OBJECT('outputs',JSON_OBJECT())),'trace-retention',?)")
        .bind(version)
        .bind(tenant)
        .bind(workflow)
        .bind(user)
        .execute(pool)
        .await
        .expect("trace workflow version");
    for execution in [unprotected_execution, protected_execution] {
        sqlx::query("INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,input_json,context_json,context_base_json,context_version,session_context_version,started_at,ended_at) VALUES(?,?,?,?,?,'manual','succeeded',JSON_OBJECT(),JSON_OBJECT(),JSON_OBJECT(),0,0,DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY),DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY))")
            .bind(execution)
            .bind(tenant)
            .bind(workflow)
            .bind(version)
            .bind(Uuid::now_v7())
            .execute(pool)
            .await
            .expect("old trace execution");
        client
            .query("INSERT INTO workflow_trace_events SELECT toUUID(?),toUUID(?)")
            .bind(tenant.to_string())
            .bind(execution.to_string())
            .execute()
            .await
            .expect("trace event");
    }
    sqlx::query("INSERT INTO checkpoints(id,tenant_id,execution_id,sequence_number,checkpoint_type,state_hash,payload_json) VALUES(?,?,?,1,'execution_start','trace-checkpoint',JSON_OBJECT())")
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(protected_execution)
        .execute(pool)
        .await
        .expect("protected trace checkpoint");
    sqlx::query("INSERT INTO retention_policies(tenant_id,data_type,retention_days,updated_by) VALUES(?,'trace',1,?)")
        .bind(tenant)
        .bind(user)
        .execute(pool)
        .await
        .expect("trace retention policy");
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,requested_by) VALUES(?,?,FALSE,?)",
    )
    .bind(run)
    .bind(tenant)
    .bind(user)
    .execute(pool)
    .await
    .expect("trace retention run");

    let unavailable = RetentionProcessor::new(pool.clone(), None, None);
    assert_eq!(
        unavailable
            .process_batch()
            .await
            .expect("failed trace segment"),
        0
    );
    let states = item_states(pool, run).await;
    assert_eq!(states.get("blocked").copied(), Some(1));
    assert_eq!(states.get("failed").copied(), Some(1));
    sqlx::query("UPDATE retention_runs SET available_at=CURRENT_TIMESTAMP(6) WHERE id=?")
        .bind(run)
        .execute(pool)
        .await
        .expect("make trace retry available");

    let recovered = RetentionProcessor::new(pool.clone(), None, Some(client.clone()));
    assert_eq!(
        recovered
            .process_batch()
            .await
            .expect("recover trace segment"),
        1
    );
    let status: String = sqlx::query_scalar("SELECT status FROM retention_runs WHERE id=?")
        .bind(run)
        .fetch_one(pool)
        .await
        .expect("trace run status");
    assert_eq!(status, "completed");
    let unprotected_count: u64 = client
        .query("SELECT count() FROM workflow_trace_events WHERE tenant_id=toUUID(?) AND execution_id=toUUID(?)")
        .bind(tenant.to_string())
        .bind(unprotected_execution.to_string())
        .fetch_one()
        .await
        .expect("unprotected trace count");
    let protected_count: u64 = client
        .query("SELECT count() FROM workflow_trace_events WHERE tenant_id=toUUID(?) AND execution_id=toUUID(?)")
        .bind(tenant.to_string())
        .bind(protected_execution.to_string())
        .fetch_one()
        .await
        .expect("protected trace count");
    assert_eq!(unprotected_count, 0);
    assert_eq!(protected_count, 1);
}

async fn verify_message_and_evaluation_cleanup(pool: &MySqlPool) {
    let tenant = Uuid::now_v7();
    let user = Uuid::now_v7();
    let department = Uuid::now_v7();
    let workflow = Uuid::now_v7();
    let workflow_version = Uuid::now_v7();
    seed_actor(pool, tenant, user).await;
    sqlx::query("INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES(?,?,'Root','root',TRUE)")
        .bind(department)
        .bind(tenant)
        .execute(pool)
        .await
        .expect("cleanup department");
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES(?,?,'Retention Cleanup',?,?)")
        .bind(workflow)
        .bind(tenant)
        .bind(user)
        .bind(department)
        .execute(pool)
        .await
        .expect("cleanup workflow");
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,created_by) VALUES(?,?,?,1,1,'4.0',JSON_OBJECT('schemaVersion','4.0','start',JSON_OBJECT('inputs',JSON_OBJECT(),'contexts',JSON_OBJECT()),'nodes',JSON_ARRAY(),'connections',JSON_ARRAY(),'end',JSON_OBJECT('outputs',JSON_OBJECT())),'cleanup-version',?)")
        .bind(workflow_version)
        .bind(tenant)
        .bind(workflow)
        .bind(user)
        .execute(pool)
        .await
        .expect("cleanup workflow version");

    let environment = Uuid::now_v7();
    let application = Uuid::now_v7();
    let deployment = Uuid::now_v7();
    let session = Uuid::now_v7();
    let message = Uuid::now_v7();
    sqlx::query("INSERT INTO workflow_environments(id,tenant_id,code,name,is_builtin) VALUES(?,?,'retention','Retention',FALSE)")
        .bind(environment)
        .bind(tenant)
        .execute(pool)
        .await
        .expect("cleanup environment");
    sqlx::query("INSERT INTO applications(id,tenant_id,workflow_id,name,slug,visibility,owner_user_id,owner_department_id,status) VALUES(?,?,?,'Retention App',?,'company',?,?, 'active')")
        .bind(application)
        .bind(tenant)
        .bind(workflow)
        .bind(format!("retention-{application}"))
        .bind(user)
        .bind(department)
        .execute(pool)
        .await
        .expect("cleanup application");
    sqlx::query("INSERT INTO application_deployments(id,tenant_id,application_id,workflow_version_id,environment_id,sequence_number,created_by) VALUES(?,?,?,?,?,1,?)")
        .bind(deployment)
        .bind(tenant)
        .bind(application)
        .bind(workflow_version)
        .bind(environment)
        .bind(user)
        .execute(pool)
        .await
        .expect("cleanup deployment");
    sqlx::query("INSERT INTO application_sessions(id,tenant_id,application_id,application_deployment_id,workflow_version_id,version_policy,status,created_by_user_id) VALUES(?,?,?,?,?,'pinned','closed',?)")
        .bind(session)
        .bind(tenant)
        .bind(application)
        .bind(deployment)
        .bind(workflow_version)
        .bind(user)
        .execute(pool)
        .await
        .expect("closed cleanup session");
    sqlx::query("INSERT INTO application_messages(id,tenant_id,session_id,sequence_number,role,created_at) VALUES(?,?,?,1,'user',DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY))")
        .bind(message)
        .bind(tenant)
        .bind(session)
        .execute(pool)
        .await
        .expect("old application message");

    let dataset = Uuid::now_v7();
    let dataset_version = Uuid::now_v7();
    let profile = Uuid::now_v7();
    let profile_version = Uuid::now_v7();
    sqlx::query("INSERT INTO datasets(id,tenant_id,name,owner_user_id,owner_department_id,visibility) VALUES(?,?, 'Retention Dataset',?,?, 'company')")
        .bind(dataset)
        .bind(tenant)
        .bind(user)
        .bind(department)
        .execute(pool)
        .await
        .expect("cleanup dataset");
    sqlx::query("INSERT INTO dataset_versions(id,tenant_id,dataset_id,version_number,source_revision,content_hash,case_count,created_by) VALUES(?,?,?,1,1,REPEAT('a',64),0,?)")
        .bind(dataset_version)
        .bind(tenant)
        .bind(dataset)
        .bind(user)
        .execute(pool)
        .await
        .expect("cleanup dataset version");
    sqlx::query("INSERT INTO evaluation_profiles(id,tenant_id,name,owner_user_id,owner_department_id,visibility) VALUES(?,?, 'Retention Profile',?,?, 'company')")
        .bind(profile)
        .bind(tenant)
        .bind(user)
        .bind(department)
        .execute(pool)
        .await
        .expect("cleanup profile");
    sqlx::query("INSERT INTO evaluation_profile_versions(id,tenant_id,profile_id,version_number,content_hash,created_by) VALUES(?,?,?,1,REPEAT('b',64),?)")
        .bind(profile_version)
        .bind(tenant)
        .bind(profile)
        .bind(user)
        .execute(pool)
        .await
        .expect("cleanup profile version");
    let protected_run = Uuid::now_v7();
    let candidate_run = Uuid::now_v7();
    let deletable_run = Uuid::now_v7();
    for run in [protected_run, candidate_run, deletable_run] {
        sqlx::query("INSERT INTO evaluation_runs(id,tenant_id,name,workflow_version_id,dataset_version_id,evaluation_profile_version_id,parameters_json,status,created_by,owner_department_id,visibility,created_at,completed_at) VALUES(?,?,?, ?,?,?,JSON_OBJECT(),'completed',?,?, 'company',DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY),DATE_SUB(CURRENT_TIMESTAMP(6),INTERVAL 10 DAY))")
            .bind(run)
            .bind(tenant)
            .bind(format!("Retention Run {run}"))
            .bind(workflow_version)
            .bind(dataset_version)
            .bind(profile_version)
            .bind(user)
            .bind(department)
            .execute(pool)
            .await
            .expect("old evaluation run");
    }
    sqlx::query("INSERT INTO evaluation_comparisons(id,tenant_id,name,baseline_run_id,candidate_run_id,created_by) VALUES(?,?, 'Retention Comparison',?,?,?)")
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(protected_run)
        .bind(candidate_run)
        .bind(user)
        .execute(pool)
        .await
        .expect("evaluation comparison reference");
    sqlx::query("INSERT INTO evaluation_metrics(tenant_id,evaluation_run_id,metric_key,metric_value) VALUES(?,?, 'score',0.5)")
        .bind(tenant)
        .bind(deletable_run)
        .execute(pool)
        .await
        .expect("evaluation metric");

    let retention_run = Uuid::now_v7();
    sqlx::query("INSERT INTO retention_policies(tenant_id,data_type,retention_days,updated_by) VALUES(?,'application_message',1,?),(?,'evaluation_report',1,?)")
        .bind(tenant)
        .bind(user)
        .bind(tenant)
        .bind(user)
        .execute(pool)
        .await
        .expect("message and evaluation policies");
    sqlx::query(
        "INSERT INTO retention_runs(id,tenant_id,dry_run,requested_by) VALUES(?,?,FALSE,?)",
    )
    .bind(retention_run)
    .bind(tenant)
    .bind(user)
    .execute(pool)
    .await
    .expect("message and evaluation retention run");
    let processor = RetentionProcessor::new(pool.clone(), None, None);
    assert_eq!(
        processor
            .process_batch()
            .await
            .expect("message and evaluation cleanup"),
        2
    );
    let message_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM application_messages WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(message)
            .fetch_one(pool)
            .await
            .expect("message deletion");
    assert_eq!(message_exists, 0);
    let deletable_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM evaluation_runs WHERE tenant_id=? AND id=?")
            .bind(tenant)
            .bind(deletable_run)
            .fetch_one(pool)
            .await
            .expect("evaluation deletion");
    assert_eq!(deletable_exists, 0);
    let metric_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM evaluation_metrics WHERE tenant_id=? AND evaluation_run_id=?",
    )
    .bind(tenant)
    .bind(deletable_run)
    .fetch_one(pool)
    .await
    .expect("evaluation metric deletion");
    assert_eq!(metric_exists, 0);
    let states = item_states(pool, retention_run).await;
    assert_eq!(states.get("deleted").copied(), Some(2));
    assert_eq!(states.get("blocked").copied(), Some(2));
}

async fn seed_actor(pool: &MySqlPool, tenant: Uuid, user: Uuid) {
    sqlx::query("INSERT INTO tenants(id,name,normalized_name) VALUES(?,'Retention','retention')")
        .bind(tenant)
        .execute(pool)
        .await
        .expect("tenant");
    sqlx::query("INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(?,?,'retention.user','retention.user','Retention User')")
        .bind(user)
        .bind(tenant)
        .execute(pool)
        .await
        .expect("user");
}

async fn connect_with_retry(settings: &MySqlSettings) -> MySqlPool {
    let mut last_error = None;
    for _ in 0..60 {
        match mysql::connect(settings).await {
            Ok(pool) => return pool,
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    panic!("connect MySQL: {:#}", last_error.expect("connection error"));
}

async fn item_states(pool: &MySqlPool, run: Uuid) -> std::collections::HashMap<String, i64> {
    sqlx::query("SELECT status,COUNT(*) count FROM retention_items WHERE retention_run_id=? GROUP BY status")
        .bind(run)
        .fetch_all(pool)
        .await
        .expect("retention item states")
        .into_iter()
        .map(|row| {
            (
                row.try_get::<String, _>("status").unwrap(),
                row.try_get::<i64, _>("count").unwrap(),
            )
        })
        .collect()
}
