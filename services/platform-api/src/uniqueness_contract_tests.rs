use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use futures::StreamExt;
use object_store::ObjectStore;
use object_store::memory::InMemory;
use secrecy::SecretString;
use serde_json::json;
use sqlx::MySqlPool;
use uuid::Uuid;

use crate::{
    applications::APPLICATION_SLUG,
    config::AuthSettings,
    datasets::{
        self, CaseInput, CreateCaseRequest, CreateDatasetRequest, DATASET_CASE_KEY,
        ImportCasesRequest, UpdateCaseRequest,
    },
    error::{AppError, UniqueConstraint, map_unique},
    external_resources::{KNOWLEDGE_EXTERNAL_ID, MEMORY_EXTERNAL_NAMESPACE},
    iam::{DEPARTMENT_NAME, ROLE_CODE, USERNAME},
    mcp_control::MCP_SERVER_NAME,
    models_control::MODEL_NAME,
    sandbox_profiles::SANDBOX_PROFILE_NAME,
    security::AuthActor,
    skills_control::{
        self, CreateSkillRequest, ImportedWorkspaceEntry, PublishSkillVersionRequest, SKILL_ALIAS,
        SKILL_NAME, SKILL_PATH,
    },
    state::AppState,
    workflows::ENVIRONMENT_CODE,
};

fn auth_settings() -> AuthSettings {
    AuthSettings {
        signing_secret: SecretString::from(
            "uniqueness-contract-signing-secret-32-bytes".to_owned(),
        ),
        issuer: "agentx-test".to_owned(),
        audience: "agentx-test".to_owned(),
        access_ttl_seconds: 60,
        refresh_ttl_seconds: 60,
        change_password_ttl_seconds: 60,
        cookie_secure: false,
        login_max_failures: 5,
        login_failure_window_seconds: 60,
        login_lock_seconds: 60,
    }
}

fn actor() -> AuthActor {
    AuthActor {
        tenant_id: Uuid::parse_str("01010101-0101-0101-0101-010101010101").unwrap(),
        user_id: Uuid::parse_str("03030303-0303-0303-0303-030303030303").unwrap(),
        username: "admin".to_owned(),
        display_name: "Admin".to_owned(),
        department_id: Uuid::parse_str("02020202-0202-0202-0202-020202020202").unwrap(),
        permissions: vec![
            "dataset:manage".to_owned(),
            "dataset:view".to_owned(),
            "skill:manage".to_owned(),
            "skill:view".to_owned(),
        ],
        roles: vec!["company_admin".to_owned()],
        company_admin: true,
    }
}

fn case_input(key: &str, name: &str) -> CaseInput {
    CaseInput {
        case_key: key.to_owned(),
        name: name.to_owned(),
        input: json!({"question":"hello"}),
        expected_output: None,
        context: None,
        tags: Vec::new(),
        evaluator_override: None,
    }
}

const TENANT: &str = "UNHEX('01010101010101010101010101010101')";
const DEPARTMENT: &str = "UNHEX('02020202020202020202020202020202')";
const USER: &str = "UNHEX('03030303030303030303030303030303')";
const WORKFLOW: &str = "UNHEX('04040404040404040404040404040404')";
const DATASET: &str = "UNHEX('05050505050505050505050505050505')";
const RAG_CONNECTION: &str = "UNHEX('06060606060606060606060606060606')";
const MEMORY_CONNECTION: &str = "UNHEX('07070707070707070707070707070707')";
const MODEL_DEPLOYMENT: &str = "UNHEX('08080808080808080808080808080808')";
const SKILL: &str = "UNHEX('09090909090909090909090909090909')";

struct UniqueCase {
    name: &'static str,
    insert: String,
    constraint: UniqueConstraint,
}

struct MutableUniqueCase {
    name: &'static str,
    first: String,
    second: String,
    self_update: String,
    conflicting_update: String,
    constraint: UniqueConstraint,
}

fn mutable_case(
    name: &'static str,
    constraint: UniqueConstraint,
    first: String,
    second: String,
    self_update: &str,
    conflicting_update: &str,
) -> MutableUniqueCase {
    MutableUniqueCase {
        name,
        first,
        second,
        self_update: self_update.to_owned(),
        conflicting_update: conflicting_update.to_owned(),
        constraint,
    }
}

async fn fixtures(pool: &MySqlPool) {
    let statements = [
        format!(
            "INSERT INTO tenants(id,name,normalized_name) VALUES({TENANT},'Unique Test','unique-test')"
        ),
        format!(
            "INSERT INTO departments(id,tenant_id,name,normalized_name,is_root) VALUES({DEPARTMENT},{TENANT},'Root','root',TRUE)"
        ),
        format!(
            "INSERT INTO users(id,tenant_id,username,username_normalized,display_name,status) VALUES({USER},{TENANT},'admin','admin','Admin','active')"
        ),
        format!(
            "INSERT INTO workflows(id,tenant_id,name,owner_user_id,owner_department_id) VALUES({WORKFLOW},{TENANT},'Workflow',{USER},{DEPARTMENT})"
        ),
        format!(
            "INSERT INTO datasets(id,tenant_id,name,owner_user_id,owner_department_id) VALUES({DATASET},{TENANT},'Dataset',{USER},{DEPARTMENT})"
        ),
        format!(
            "INSERT INTO rag_connections(id,tenant_id,name,endpoint,owner_department_id,configuration_json) VALUES({RAG_CONNECTION},{TENANT},'RAG','https://rag.example',{DEPARTMENT},JSON_OBJECT())"
        ),
        format!(
            "INSERT INTO memory_connections(id,tenant_id,name,endpoint,owner_department_id,configuration_json) VALUES({MEMORY_CONNECTION},{TENANT},'Memory','https://memory.example',{DEPARTMENT},JSON_OBJECT())"
        ),
        format!(
            "INSERT INTO model_deployments(id,tenant_id,connection_name,provider_type,endpoint,owner_department_id,model_name,default_parameters) VALUES({MODEL_DEPLOYMENT},{TENANT},'Model','openai_compatible','https://model.example',{DEPARTMENT},'upstream',JSON_OBJECT())"
        ),
        format!(
            "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES({SKILL},{TENANT},'Fixture Skill','fixture-skill',{DEPARTMENT},{USER})"
        ),
    ];
    for statement in statements {
        sqlx::query(&statement).execute(pool).await.unwrap();
    }
}

fn cases() -> Vec<UniqueCase> {
    vec![
        UniqueCase {
            name: "model name",
            constraint: MODEL_NAME,
            insert: format!(
                "INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(UUID_TO_BIN(UUID()),{TENANT},'duplicate-model',{MODEL_DEPLOYMENT})"
            ),
        },
        UniqueCase {
            name: "department sibling name",
            constraint: DEPARTMENT_NAME,
            insert: format!(
                "INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name) VALUES(UUID_TO_BIN(UUID()),{TENANT},{DEPARTMENT},'Duplicate Department','duplicate department')"
            ),
        },
        UniqueCase {
            name: "username",
            constraint: USERNAME,
            insert: format!(
                "INSERT INTO users(id,tenant_id,username,username_normalized,display_name) VALUES(UUID_TO_BIN(UUID()),{TENANT},'Duplicate.User','duplicate.user','Duplicate User')"
            ),
        },
        UniqueCase {
            name: "role code",
            constraint: ROLE_CODE,
            insert: format!(
                "INSERT INTO roles(id,tenant_id,code,name,data_scope) VALUES(UUID_TO_BIN(UUID()),{TENANT},'duplicate-role','Duplicate Role','own')"
            ),
        },
        UniqueCase {
            name: "application slug",
            constraint: APPLICATION_SLUG,
            insert: format!(
                "INSERT INTO applications(id,tenant_id,workflow_id,name,slug,owner_user_id,owner_department_id) VALUES(UUID_TO_BIN(UUID()),{TENANT},{WORKFLOW},'Duplicate Application','duplicate-application',{USER},{DEPARTMENT})"
            ),
        },
        UniqueCase {
            name: "environment code",
            constraint: ENVIRONMENT_CODE,
            insert: format!(
                "INSERT INTO workflow_environments(id,tenant_id,code,name) VALUES(UUID_TO_BIN(UUID()),{TENANT},'duplicate-environment','Duplicate Environment')"
            ),
        },
        UniqueCase {
            name: "MCP server name",
            constraint: MCP_SERVER_NAME,
            insert: format!(
                "INSERT INTO mcp_servers(id,tenant_id,name,owner_department_id,created_by) VALUES(UUID_TO_BIN(UUID()),{TENANT},'Duplicate MCP',{DEPARTMENT},{USER})"
            ),
        },
        UniqueCase {
            name: "Skill name",
            constraint: SKILL_NAME,
            insert: format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UUID_TO_BIN(UUID()),{TENANT},'Duplicate Skill',CONCAT('alias-',REPLACE(UUID(),'-','')),{DEPARTMENT},{USER})"
            ),
        },
        UniqueCase {
            name: "Skill alias",
            constraint: SKILL_ALIAS,
            insert: format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UUID_TO_BIN(UUID()),{TENANT},CONCAT('Skill ',UUID()),'duplicate-skill-alias',{DEPARTMENT},{USER})"
            ),
        },
        UniqueCase {
            name: "Skill path",
            constraint: SKILL_PATH,
            insert: format!(
                "INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,name,path,path_hash,entry_type) VALUES(UUID_TO_BIN(UUID()),{TENANT},{SKILL},'duplicate.md','duplicate.md',SHA2('duplicate.md',256),'file')"
            ),
        },
        UniqueCase {
            name: "Sandbox Profile name",
            constraint: SANDBOX_PROFILE_NAME,
            insert: format!(
                "INSERT INTO sandbox_profiles(id,tenant_id,name,owner_department_id,created_by) VALUES(UUID_TO_BIN(UUID()),{TENANT},'Duplicate Sandbox',{DEPARTMENT},{USER})"
            ),
        },
        UniqueCase {
            name: "Dataset Case Key",
            constraint: DATASET_CASE_KEY,
            insert: format!(
                "INSERT INTO dataset_cases(id,tenant_id,dataset_id,case_key,name,input_json,tags_json,sort_order) VALUES(UUID_TO_BIN(UUID()),{TENANT},{DATASET},'duplicate-case','Duplicate Case',JSON_OBJECT(),JSON_ARRAY(),1)"
            ),
        },
        UniqueCase {
            name: "Knowledge external resource",
            constraint: KNOWLEDGE_EXTERNAL_ID,
            insert: format!(
                "INSERT INTO rag_resources(id,tenant_id,connection_id,name,external_resource_id,owner_department_id) VALUES(UUID_TO_BIN(UUID()),{TENANT},{RAG_CONNECTION},'Duplicate Knowledge','duplicate-external',{DEPARTMENT})"
            ),
        },
        UniqueCase {
            name: "Memory external namespace",
            constraint: MEMORY_EXTERNAL_NAMESPACE,
            insert: format!(
                "INSERT INTO memory_namespaces(id,tenant_id,connection_id,name,external_namespace,owner_department_id) VALUES(UUID_TO_BIN(UUID()),{TENANT},{MEMORY_CONNECTION},'Duplicate Memory','duplicate-namespace',{DEPARTMENT})"
            ),
        },
    ]
}

fn mutable_cases() -> Vec<MutableUniqueCase> {
    vec![
        mutable_case(
            "model name",
            MODEL_NAME,
            format!(
                "INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(UNHEX('10101010101010101010101010101010'),{TENANT},'model-first',{MODEL_DEPLOYMENT})"
            ),
            format!(
                "INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(UNHEX('11111111111111111111111111111111'),{TENANT},'model-second',{MODEL_DEPLOYMENT})"
            ),
            "UPDATE model_aliases SET alias='model-second' WHERE id=UNHEX('11111111111111111111111111111111')",
            "UPDATE model_aliases SET alias='model-first' WHERE id=UNHEX('11111111111111111111111111111111')",
        ),
        mutable_case(
            "department sibling name",
            DEPARTMENT_NAME,
            format!(
                "INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name) VALUES(UNHEX('12121212121212121212121212121212'),{TENANT},{DEPARTMENT},'Department First','department first')"
            ),
            format!(
                "INSERT INTO departments(id,tenant_id,parent_id,name,normalized_name) VALUES(UNHEX('13131313131313131313131313131313'),{TENANT},{DEPARTMENT},'Department Second','department second')"
            ),
            "UPDATE departments SET normalized_name='department second' WHERE id=UNHEX('13131313131313131313131313131313')",
            "UPDATE departments SET normalized_name='department first' WHERE id=UNHEX('13131313131313131313131313131313')",
        ),
        mutable_case(
            "MCP server name",
            MCP_SERVER_NAME,
            format!(
                "INSERT INTO mcp_servers(id,tenant_id,name,owner_department_id,created_by) VALUES(UNHEX('14141414141414141414141414141414'),{TENANT},'MCP First',{DEPARTMENT},{USER})"
            ),
            format!(
                "INSERT INTO mcp_servers(id,tenant_id,name,owner_department_id,created_by) VALUES(UNHEX('15151515151515151515151515151515'),{TENANT},'MCP Second',{DEPARTMENT},{USER})"
            ),
            "UPDATE mcp_servers SET name='MCP Second' WHERE id=UNHEX('15151515151515151515151515151515')",
            "UPDATE mcp_servers SET name='MCP First' WHERE id=UNHEX('15151515151515151515151515151515')",
        ),
        mutable_case(
            "Skill name",
            SKILL_NAME,
            format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UNHEX('16161616161616161616161616161616'),{TENANT},'Skill First','skill-first',{DEPARTMENT},{USER})"
            ),
            format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UNHEX('17171717171717171717171717171717'),{TENANT},'Skill Second','skill-second',{DEPARTMENT},{USER})"
            ),
            "UPDATE skills SET name='Skill Second' WHERE id=UNHEX('17171717171717171717171717171717')",
            "UPDATE skills SET name='Skill First' WHERE id=UNHEX('17171717171717171717171717171717')",
        ),
        mutable_case(
            "Skill alias",
            SKILL_ALIAS,
            format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UNHEX('18181818181818181818181818181818'),{TENANT},'Alias First','alias-first',{DEPARTMENT},{USER})"
            ),
            format!(
                "INSERT INTO skills(id,tenant_id,name,alias,owner_department_id,created_by) VALUES(UNHEX('19191919191919191919191919191919'),{TENANT},'Alias Second','alias-second',{DEPARTMENT},{USER})"
            ),
            "UPDATE skills SET alias='alias-second' WHERE id=UNHEX('19191919191919191919191919191919')",
            "UPDATE skills SET alias='alias-first' WHERE id=UNHEX('19191919191919191919191919191919')",
        ),
        mutable_case(
            "Skill path",
            SKILL_PATH,
            format!(
                "INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,name,path,path_hash,entry_type) VALUES(UNHEX('20202020202020202020202020202020'),{TENANT},{SKILL},'first.md','first.md',SHA2('first.md',256),'file')"
            ),
            format!(
                "INSERT INTO skill_workspace_entries(id,tenant_id,skill_id,name,path,path_hash,entry_type) VALUES(UNHEX('21212121212121212121212121212121'),{TENANT},{SKILL},'second.md','second.md',SHA2('second.md',256),'file')"
            ),
            "UPDATE skill_workspace_entries SET path_hash=SHA2('second.md',256) WHERE id=UNHEX('21212121212121212121212121212121')",
            "UPDATE skill_workspace_entries SET path_hash=SHA2('first.md',256) WHERE id=UNHEX('21212121212121212121212121212121')",
        ),
        mutable_case(
            "Sandbox Profile name",
            SANDBOX_PROFILE_NAME,
            format!(
                "INSERT INTO sandbox_profiles(id,tenant_id,name,owner_department_id,created_by) VALUES(UNHEX('22222222222222222222222222222222'),{TENANT},'Sandbox First',{DEPARTMENT},{USER})"
            ),
            format!(
                "INSERT INTO sandbox_profiles(id,tenant_id,name,owner_department_id,created_by) VALUES(UNHEX('23232323232323232323232323232323'),{TENANT},'Sandbox Second',{DEPARTMENT},{USER})"
            ),
            "UPDATE sandbox_profiles SET name='Sandbox Second' WHERE id=UNHEX('23232323232323232323232323232323')",
            "UPDATE sandbox_profiles SET name='Sandbox First' WHERE id=UNHEX('23232323232323232323232323232323')",
        ),
        mutable_case(
            "Dataset Case Key",
            DATASET_CASE_KEY,
            format!(
                "INSERT INTO dataset_cases(id,tenant_id,dataset_id,case_key,name,input_json,tags_json,sort_order) VALUES(UNHEX('24242424242424242424242424242424'),{TENANT},{DATASET},'case-first','First',JSON_OBJECT(),JSON_ARRAY(),1)"
            ),
            format!(
                "INSERT INTO dataset_cases(id,tenant_id,dataset_id,case_key,name,input_json,tags_json,sort_order) VALUES(UNHEX('25252525252525252525252525252525'),{TENANT},{DATASET},'case-second','Second',JSON_OBJECT(),JSON_ARRAY(),2)"
            ),
            "UPDATE dataset_cases SET case_key='case-second' WHERE id=UNHEX('25252525252525252525252525252525')",
            "UPDATE dataset_cases SET case_key='case-first' WHERE id=UNHEX('25252525252525252525252525252525')",
        ),
    ]
}

#[tokio::test]
async fn all_user_unique_constraints_map_concurrent_losers_to_field_conflicts() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    fixtures(&pool).await;

    for case in cases() {
        let (left, right) = tokio::join!(
            sqlx::query(&case.insert).execute(&pool),
            sqlx::query(&case.insert).execute(&pool),
        );
        let (successes, errors): (Vec<_>, Vec<_>) =
            [left, right].into_iter().partition(Result::is_ok);
        assert_eq!(successes.len(), 1, "{} must have one winner", case.name);
        let error = errors.into_iter().next().unwrap().unwrap_err();
        let mapped = map_unique(error, &[case.constraint]);
        assert_eq!(mapped.status, StatusCode::CONFLICT, "{} status", case.name);
        assert_eq!(mapped.code, case.constraint.code, "{} code", case.name);
        assert_eq!(mapped.fields.len(), 1, "{} field count", case.name);
        assert_eq!(
            mapped.fields[0].field, case.constraint.field,
            "{} field",
            case.name
        );
        assert_eq!(
            mapped.fields[0].code, case.constraint.code,
            "{} field code",
            case.name
        );
    }
}

#[tokio::test]
async fn unchanged_unique_values_update_successfully_and_other_values_conflict() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    fixtures(&pool).await;

    for case in mutable_cases() {
        sqlx::query(&case.first).execute(&pool).await.unwrap();
        sqlx::query(&case.second).execute(&pool).await.unwrap();
        sqlx::query(&case.self_update).execute(&pool).await.unwrap();
        let error = sqlx::query(&case.conflicting_update)
            .execute(&pool)
            .await
            .unwrap_err();
        let mapped = map_unique(error, &[case.constraint]);
        assert_eq!(mapped.status, StatusCode::CONFLICT, "{} status", case.name);
        assert_eq!(mapped.code, case.constraint.code, "{} code", case.name);
        assert_eq!(
            mapped.fields[0].field, case.constraint.field,
            "{} field",
            case.name
        );
    }
}

#[tokio::test]
async fn an_unregistered_unique_index_is_internal_not_generic_conflict() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    let storage_key = "unknown-unique-index";
    for _ in 0..2 {
        let result = sqlx::query("INSERT INTO artifacts(id,tenant_id,content_type,size_bytes,sha256,storage_key) VALUES(UUID_TO_BIN(UUID()),UUID_TO_BIN(UUID()),'text/plain',1,REPEAT('a',64),?)")
            .bind(storage_key)
            .execute(&pool)
            .await;
        if let Err(error) = result {
            let mapped = AppError::from(error);
            assert_eq!(mapped.status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(mapped.code, "INTERNAL_ERROR");
            assert!(mapped.fields.is_empty());
            return;
        }
    }
    panic!("second Artifact insert should violate the unregistered index");
}

#[tokio::test]
async fn dataset_duplicate_contract_covers_create_update_import_and_lines() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    fixtures(&pool).await;
    let state = AppState::new(pool, auth_settings());
    let actor = actor();
    let (_, Json(dataset)) = datasets::create_dataset(
        State(state.clone()),
        actor.clone(),
        Json(CreateDatasetRequest {
            name: "Contract Dataset".to_owned(),
            description: None,
            visibility: "private".to_owned(),
        }),
    )
    .await
    .unwrap();

    let (_, Json(first)) = datasets::create_case(
        State(state.clone()),
        actor.clone(),
        Path(dataset.id),
        Json(CreateCaseRequest {
            expected_revision: 0,
            case: case_input("first", "First"),
        }),
    )
    .await
    .unwrap();
    let duplicate = datasets::create_case(
        State(state.clone()),
        actor.clone(),
        Path(dataset.id),
        Json(CreateCaseRequest {
            expected_revision: 1,
            case: case_input("first", "Duplicate"),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (
            duplicate.status,
            duplicate.code,
            duplicate.fields[0].field.as_str()
        ),
        (StatusCode::CONFLICT, "DATASET_CASE_KEY_EXISTS", "caseKey")
    );

    let (_, Json(second)) = datasets::create_case(
        State(state.clone()),
        actor.clone(),
        Path(dataset.id),
        Json(CreateCaseRequest {
            expected_revision: 1,
            case: case_input("second", "Second"),
        }),
    )
    .await
    .unwrap();
    let unchanged = datasets::update_case(
        State(state.clone()),
        actor.clone(),
        Path((dataset.id, second.id)),
        Json(UpdateCaseRequest {
            expected_revision: 2,
            version: second.version,
            case: case_input("second", "Second unchanged"),
        }),
    )
    .await
    .unwrap();
    assert_eq!(unchanged.0.case_key, "second");
    let conflicting = datasets::update_case(
        State(state.clone()),
        actor.clone(),
        Path((dataset.id, second.id)),
        Json(UpdateCaseRequest {
            expected_revision: 3,
            version: second.version + 1,
            case: case_input("first", "Second conflict"),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (conflicting.code, conflicting.fields[0].field.as_str()),
        ("DATASET_CASE_KEY_EXISTS", "caseKey")
    );

    let internal = datasets::import_cases(State(state.clone()), actor.clone(), Path(dataset.id), Json(ImportCasesRequest { expected_revision: 3, format: "jsonl".to_owned(), content: "{\"caseKey\":\"inside\",\"name\":\"One\",\"input\":{}}\n{\"caseKey\":\"inside\",\"name\":\"Two\",\"input\":{}}".to_owned() })).await.unwrap_err();
    assert_eq!(
        (internal.code, internal.fields[0].field.as_str()),
        ("DATASET_CASE_KEY_EXISTS", "file")
    );
    assert_eq!(internal.details.as_ref().unwrap()["caseKey"], "inside");
    assert_eq!(internal.details.as_ref().unwrap()["line"], 2);
    assert_eq!(internal.details.as_ref().unwrap()["firstLine"], 1);

    let database = datasets::import_cases(State(state), actor, Path(dataset.id), Json(ImportCasesRequest { expected_revision: 3, format: "jsonl".to_owned(), content: "{\"caseKey\":\"new\",\"name\":\"New\",\"input\":{}}\n{\"caseKey\":\"first\",\"name\":\"Existing\",\"input\":{}}".to_owned() })).await.unwrap_err();
    assert_eq!(
        (database.code, database.fields[0].field.as_str()),
        ("DATASET_CASE_KEY_EXISTS", "file")
    );
    assert_eq!(database.details.as_ref().unwrap()["caseKey"], "first");
    assert_eq!(database.details.as_ref().unwrap()["line"], 2);
    assert_eq!(first.case_key, "first");
}

#[tokio::test]
async fn skill_publish_reuses_identical_version_without_duplicate_rows() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    fixtures(&pool).await;
    let state = AppState::new(pool.clone(), auth_settings()).with_m2(
        agentx_infrastructure::credential::CredentialKeyring::from_json(
            "test".to_owned(),
            &SecretString::from(
                r#"{"keys":{"test":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#.to_owned(),
            ),
        )
        .unwrap(),
        Some(Arc::new(InMemory::new())),
        crate::config::ConnectionSettings {
            timeout_seconds: 1,
            max_concurrency: 1,
            allow_private_networks: false,
            allowed_hosts: Vec::new(),
            allowed_cidrs: Vec::new(),
        },
    );
    let actor = actor();
    let (_, Json(skill)) = skills_control::create_skill(
        State(state.clone()),
        actor.clone(),
        Json(CreateSkillRequest {
            name: "Publish Contract Skill".to_owned(),
            alias: "publish-contract-skill".to_owned(),
            description: "Reusable instructions".to_owned(),
            owner_department_id: actor.department_id,
        }),
    )
    .await
    .unwrap();
    let request = || PublishSkillVersionRequest {
        expected_revision: skill.draft_revision,
        dependencies: Vec::new(),
    };
    let (first_status, Json(first)) = skills_control::create_version(
        State(state.clone()),
        actor.clone(),
        Path(skill.id),
        Json(request()),
    )
    .await
    .unwrap();
    let (second_status, Json(second)) =
        skills_control::create_version(State(state), actor, Path(skill.id), Json(request()))
            .await
            .unwrap();
    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(first.id, second.id);
    let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skill_versions WHERE skill_id=?")
        .bind(skill.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let files: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM skill_version_files WHERE skill_version_id=?")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let dependencies: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM skill_dependencies WHERE skill_version_id=?")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((versions, files, dependencies), (1, 1, 0));
}

#[tokio::test]
async fn skill_import_artifact_generation_failure_compensates_metadata_objects_and_quota() {
    let _container_guard = crate::TESTCONTAINER_LOCK.lock().await;
    let (_container, pool) = crate::migration_tests::start_mysql().await;
    agentx_infrastructure::mysql::run_migrations(&pool)
        .await
        .unwrap();
    fixtures(&pool).await;
    let store = Arc::new(InMemory::new());
    let state = AppState::new(pool.clone(), auth_settings()).with_m2(
        agentx_infrastructure::credential::CredentialKeyring::from_json(
            "test".to_owned(),
            &SecretString::from(
                r#"{"keys":{"test":"YWdlbnR4LWxvY2FsLWNyZWRlbnRpYWwta2V5LTAwMDE="}}"#.to_owned(),
            ),
        )
        .unwrap(),
        Some(store.clone()),
        crate::config::ConnectionSettings {
            timeout_seconds: 1,
            max_concurrency: 1,
            allow_private_networks: false,
            allowed_hosts: Vec::new(),
            allowed_cidrs: Vec::new(),
        },
    );
    let baseline_artifacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM artifacts WHERE tenant_id=? AND deleted_at IS NULL",
    )
    .bind(actor().tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let entries = vec![
        ImportedWorkspaceEntry {
            path: "SKILL.md".to_owned(),
            mime_type: "text/markdown".to_owned(),
            content: Some(b"# Skill".to_vec()),
        },
        ImportedWorkspaceEntry {
            path: "guide.md".to_owned(),
            mime_type: "text/markdown".to_owned(),
            content: Some(b"# Guide".to_vec()),
        },
    ];
    let error = skills_control::store_import_artifacts(&state, actor().tenant_id, entries, Some(1))
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL_ERROR");
    let available: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM artifacts WHERE tenant_id=? AND deleted_at IS NULL",
    )
    .bind(actor().tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let net_usage: rust_decimal::Decimal = sqlx::query_scalar("SELECT COALESCE(SUM(amount),0) FROM quota_usage_ledger WHERE tenant_id=? AND dimension_key='artifact_bytes'").bind(actor().tenant_id).fetch_one(&pool).await.unwrap();
    let active_reservations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quota_reservations WHERE tenant_id=? AND dimension_key='artifact_bytes' AND status='active'").bind(actor().tenant_id).fetch_one(&pool).await.unwrap();
    let objects = store.list(None).collect::<Vec<_>>().await;
    assert_eq!(available, baseline_artifacts);
    assert_eq!(net_usage, rust_decimal::Decimal::ZERO);
    assert_eq!(active_reservations, 0);
    assert!(objects.is_empty());
}
