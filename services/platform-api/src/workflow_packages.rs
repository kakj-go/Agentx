use std::collections::{BTreeMap, BTreeSet};

use agentx_domain::{
    EditorDocument, WorkflowDefinition, canonical_content_hash, validate_editor_document,
};
use agentx_runtime::{CompileContext, WorkflowCompiler};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    control_common::{audit, outbox, require_workflow_access, validate_name},
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPackageManifest {
    pub package_schema_version: String,
    pub workflow_schema_version: String,
    pub name: String,
    pub content_hash: String,
    pub signature_algorithm: String,
    pub signing_key_id: String,
    pub signature: String,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeLock {
    pub node_type: String,
    pub version: u32,
    pub manifest_hash: String,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBindingPlaceholder {
    pub id: String,
    pub node_id: String,
    pub reference_index: usize,
    pub resource_type: String,
    pub operation: String,
    pub binding_role: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPackage {
    pub manifest: WorkflowPackageManifest,
    pub workflow_definition: Value,
    pub editor_document: Value,
    pub node_lock: Vec<NodeLock>,
    pub sub_workflow_references: Vec<Uuid>,
    pub resource_binding_placeholders: Vec<ResourceBindingPlaceholder>,
    pub api_binding: Value,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBindingTarget {
    pub resource_id: Uuid,
    pub resource_version_id: Option<Uuid>,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportWorkflowPackageRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub package: WorkflowPackage,
    #[serde(default)]
    pub resource_bindings: BTreeMap<String, ResourceBindingTarget>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportedWorkflowResponse {
    pub workflow_id: Uuid,
    pub draft_id: Uuid,
    pub definition_hash: String,
}

#[utoipa::path(get, path = "/api/v1/workflows/{id}/export", params(("id" = Uuid, Path)))]
pub async fn export_workflow_package(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<WorkflowPackage>> {
    actor.require("workflow:view")?;
    require_workflow_access(&state.pool, &actor, id, false).await?;
    let row = sqlx::query("SELECT w.name,d.definition_json,d.editor_json FROM workflows w JOIN workflow_drafts d ON d.workflow_id=w.id AND d.tenant_id=w.tenant_id WHERE w.tenant_id=? AND w.id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let name: String = row.try_get("name")?;
    let mut definition: Value = row.try_get("definition_json")?;
    let editor_document: Value = row
        .try_get::<Option<Value>, _>("editor_json")?
        .unwrap_or_else(|| json!({}));
    let placeholders = sanitize_resource_references(&mut definition)?;
    let parsed: WorkflowDefinition =
        serde_json::from_value(definition.clone()).map_err(|error| {
            AppError::unprocessable("INVALID_WORKFLOW_DEFINITION", error.to_string())
        })?;
    let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
    let mut locks = Vec::new();
    let mut seen = BTreeSet::new();
    for node in &parsed.nodes {
        if !seen.insert((node.node_type.clone(), node.type_version)) {
            continue;
        }
        let manifest = registry
            .get(&node.node_type, node.type_version)
            .ok_or_else(|| {
                AppError::unprocessable(
                    "NODE_MANIFEST_MISSING",
                    format!("{}@{}", node.node_type, node.type_version),
                )
            })?;
        locks.push(NodeLock {
            node_type: node.node_type.clone(),
            version: node.type_version,
            manifest_hash: canonical_content_hash(
                &serde_json::to_value(manifest).map_err(AppError::internal)?,
            )
            .map_err(AppError::internal)?,
        });
    }
    let sub_workflow_references = parsed
        .nodes
        .iter()
        .filter_map(|node| {
            node.parameters
                .get("workflowVersionId")
                .and_then(Value::as_str)
        })
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let output_properties = parsed
        .end
        .outputs
        .iter()
        .map(|(key, output)| (key.clone(), output.schema.clone()))
        .collect::<serde_json::Map<_, _>>();
    let required = parsed
        .end
        .outputs
        .iter()
        .filter(|(_, output)| output.required)
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let api_binding = json!({
        "requestSchema": parsed.start.inputs,
        "responseSchema":{"type":"object","properties":output_properties,"required":required,"additionalProperties":false},
        "chatbox":{"textInput":"question","fileInput":"attachments","answerOutput":"answer","attachmentsOutput":"attachments","citationsOutput":"citations"}
    });
    let content_hash = package_content_hash(
        &definition,
        &editor_document,
        &locks,
        &sub_workflow_references,
        &placeholders,
        &api_binding,
    )?;
    let (signing_key_id, signature) = sign_package_hash(&state, &content_hash)?;
    Ok(Json(WorkflowPackage {
        manifest: WorkflowPackageManifest {
            package_schema_version: "1.0".into(),
            workflow_schema_version: "4.0".into(),
            name,
            content_hash,
            signature_algorithm: "hmac-sha256".into(),
            signing_key_id,
            signature,
        },
        workflow_definition: definition,
        editor_document,
        node_lock: locks,
        sub_workflow_references,
        resource_binding_placeholders: placeholders,
        api_binding,
    }))
}

#[utoipa::path(post, path = "/api/v1/workflows/import", request_body = ImportWorkflowPackageRequest)]
pub async fn import_workflow_package(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(mut input): Json<ImportWorkflowPackageRequest>,
) -> AppResult<(StatusCode, Json<ImportedWorkflowResponse>)> {
    actor.require("workflow:create")?;
    validate_name(&input.name, 160)?;
    if !matches!(
        input.visibility.as_str(),
        "private" | "department" | "company"
    ) {
        return Err(AppError::bad_request(
            "INVALID_VISIBILITY",
            "Visibility is invalid",
        ));
    }
    if input.package.manifest.package_schema_version != "1.0"
        || input.package.manifest.workflow_schema_version != "4.0"
    {
        return Err(AppError::unprocessable(
            "WORKFLOW_PACKAGE_VERSION_UNSUPPORTED",
            "Only Workflow Package 1.0 with Definition 4.0 is supported",
        ));
    }
    let expected_hash = package_content_hash(
        &input.package.workflow_definition,
        &input.package.editor_document,
        &input.package.node_lock,
        &input.package.sub_workflow_references,
        &input.package.resource_binding_placeholders,
        &input.package.api_binding,
    )?;
    if expected_hash != input.package.manifest.content_hash {
        return Err(AppError::unprocessable(
            "WORKFLOW_PACKAGE_INTEGRITY_FAILED",
            "Workflow Package content hash does not match",
        ));
    }
    verify_package_signature(&state, &input.package.manifest)?;
    apply_resource_bindings(
        &mut input.package.workflow_definition,
        &input.package.resource_binding_placeholders,
        &input.resource_bindings,
    )?;
    for placeholder in &input.package.resource_binding_placeholders {
        let target = input
            .resource_bindings
            .get(&placeholder.id)
            .ok_or_else(|| {
                AppError::unprocessable("RESOURCE_BINDING_REQUIRED", placeholder.id.clone())
            })?;
        crate::grants::validate_import_resource_binding(
            &state.pool,
            actor.tenant_id,
            &placeholder.resource_type,
            target.resource_id,
            target.resource_version_id,
        )
        .await?;
    }
    let definition: WorkflowDefinition =
        serde_json::from_value(input.package.workflow_definition.clone()).map_err(|error| {
            AppError::unprocessable("INVALID_WORKFLOW_DEFINITION", error.to_string())
        })?;
    let editor: EditorDocument = serde_json::from_value(input.package.editor_document.clone())
        .map_err(|error| AppError::unprocessable("INVALID_EDITOR_DOCUMENT", error.to_string()))?;
    let editor_issues = validate_editor_document(&definition, &editor);
    if !editor_issues.is_empty() {
        return Err(AppError::unprocessable(
            "INVALID_EDITOR_DOCUMENT",
            editor_issues[0].message.clone(),
        ));
    }
    let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
    for lock in &input.package.node_lock {
        let manifest = registry.get(&lock.node_type, lock.version).ok_or_else(|| {
            AppError::unprocessable(
                "NODE_MANIFEST_MISSING",
                format!("{}@{}", lock.node_type, lock.version),
            )
        })?;
        let hash =
            canonical_content_hash(&serde_json::to_value(manifest).map_err(AppError::internal)?)
                .map_err(AppError::internal)?;
        if hash != lock.manifest_hash {
            return Err(AppError::unprocessable(
                "NODE_MANIFEST_LOCK_MISMATCH",
                format!("{}@{}", lock.node_type, lock.version),
            ));
        }
    }
    for version_id in &input.package.sub_workflow_references {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_versions WHERE tenant_id=? AND id=?)",
        )
        .bind(actor.tenant_id)
        .bind(version_id)
        .fetch_one(&state.pool)
        .await?;
        if !exists {
            return Err(AppError::unprocessable(
                "SUBWORKFLOW_DEPENDENCY_MISSING",
                version_id.to_string(),
            ));
        }
    }
    WorkflowCompiler::new(&registry)
        .compile(&definition, &CompileContext::default())
        .map_err(|error| {
            AppError::unprocessable(
                "WORKFLOW_PACKAGE_COMPILE_FAILED",
                serde_json::to_string(&error.issues).unwrap_or_default(),
            )
        })?;
    let definition_value = serde_json::to_value(&definition).map_err(AppError::internal)?;
    let editor_value = serde_json::to_value(&editor).map_err(AppError::internal)?;
    let definition_hash = canonical_content_hash(&definition_value).map_err(AppError::internal)?;
    let editor_hash = canonical_content_hash(&editor_value).map_err(AppError::internal)?;
    let workflow_id = Uuid::now_v7();
    let draft_id = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,?,?,?,?)")
        .bind(workflow_id).bind(actor.tenant_id).bind(&input.name).bind(&input.description).bind(&input.visibility).bind(actor.user_id).bind(actor.department_id)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)")
        .bind(Uuid::now_v7())
        .bind(actor.tenant_id)
        .bind(workflow_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,revision,definition_json,editor_json,content_hash,editor_hash,updated_by) VALUES(?,?,?,'4.0',0,?,?,?,?,?)")
        .bind(draft_id).bind(actor.tenant_id).bind(workflow_id).bind(definition_value).bind(editor_value).bind(&definition_hash).bind(editor_hash).bind(actor.user_id)
        .execute(&mut *tx).await?;
    audit(
        &mut tx,
        &actor,
        "workflow.imported",
        "workflow",
        workflow_id,
        json!({"packageHash":input.package.manifest.content_hash}),
    )
    .await?;
    outbox(
        &mut tx,
        &actor,
        "WorkflowImported",
        "workflow",
        workflow_id,
        json!({"workflowId":workflow_id}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(ImportedWorkflowResponse {
            workflow_id,
            draft_id,
            definition_hash,
        }),
    ))
}

fn sanitize_resource_references(
    definition: &mut Value,
) -> AppResult<Vec<ResourceBindingPlaceholder>> {
    let mut placeholders = Vec::new();
    let nodes = definition
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::unprocessable("INVALID_WORKFLOW_DEFINITION", "nodes must be an array")
        })?;
    for node in nodes {
        let node_id = node
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some(references) = node
            .get_mut("resourceReferences")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for (index, reference) in references.iter_mut().enumerate() {
            let placeholder_id = format!("resource.{node_id}.{index}");
            placeholders.push(ResourceBindingPlaceholder {
                id: placeholder_id,
                node_id: node_id.clone(),
                reference_index: index,
                resource_type: reference
                    .get("resourceType")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                operation: reference
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("use")
                    .into(),
                binding_role: reference
                    .get("bindingRole")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
            reference["resourceId"] = Value::String(Uuid::nil().to_string());
            reference["resourceVersionId"] = Value::Null;
        }
    }
    Ok(placeholders)
}

fn apply_resource_bindings(
    definition: &mut Value,
    placeholders: &[ResourceBindingPlaceholder],
    bindings: &BTreeMap<String, ResourceBindingTarget>,
) -> AppResult<()> {
    for placeholder in placeholders {
        let target = bindings.get(&placeholder.id).ok_or_else(|| {
            AppError::unprocessable("RESOURCE_BINDING_REQUIRED", placeholder.id.clone())
        })?;
        let reference = definition
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .and_then(|nodes| {
                nodes.iter_mut().find(|node| {
                    node.get("id").and_then(Value::as_str) == Some(&placeholder.node_id)
                })
            })
            .and_then(|node| node.get_mut("resourceReferences"))
            .and_then(Value::as_array_mut)
            .and_then(|references| references.get_mut(placeholder.reference_index))
            .ok_or_else(|| {
                AppError::unprocessable("RESOURCE_PLACEHOLDER_INVALID", placeholder.id.clone())
            })?;
        reference["resourceId"] = Value::String(target.resource_id.to_string());
        reference["resourceVersionId"] = target
            .resource_version_id
            .map_or(Value::Null, |id| Value::String(id.to_string()));
    }
    Ok(())
}

fn package_content_hash(
    definition: &Value,
    editor: &Value,
    locks: &[NodeLock],
    subworkflows: &[Uuid],
    placeholders: &[ResourceBindingPlaceholder],
    api_binding: &Value,
) -> AppResult<String> {
    canonical_content_hash(&json!({
        "workflowDefinition":definition,
        "editorDocument":editor,
        "nodeLock":locks,
        "subWorkflowReferences":subworkflows,
        "resourceBindingPlaceholders":placeholders,
        "apiBinding":api_binding
    }))
    .map_err(AppError::internal)
}

fn workflow_package_signing_key(state: &AppState) -> Vec<u8> {
    std::env::var("AGENTX_WORKFLOW_PACKAGE_SIGNING_KEY")
        .unwrap_or_else(|_| state.auth.signing_secret.expose_secret().to_owned())
        .into_bytes()
}

fn sign_package_hash(state: &AppState, content_hash: &str) -> AppResult<(String, String)> {
    let key = workflow_package_signing_key(state);
    sign_package_hash_with_key(&key, content_hash)
}

fn sign_package_hash_with_key(key: &[u8], content_hash: &str) -> AppResult<(String, String)> {
    let key_id = format!("sha256:{:x}", Sha256::digest(key));
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(AppError::internal)?;
    mac.update(content_hash.as_bytes());
    Ok((key_id, URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())))
}

fn verify_package_signature(state: &AppState, manifest: &WorkflowPackageManifest) -> AppResult<()> {
    verify_package_signature_with_key(&workflow_package_signing_key(state), manifest)
}

fn verify_package_signature_with_key(
    key: &[u8],
    manifest: &WorkflowPackageManifest,
) -> AppResult<()> {
    if manifest.signature_algorithm != "hmac-sha256" {
        return Err(AppError::unprocessable(
            "WORKFLOW_PACKAGE_SIGNATURE_UNSUPPORTED",
            "Only hmac-sha256 Workflow Package signatures are supported",
        ));
    }
    let expected_key_id = format!("sha256:{:x}", Sha256::digest(key));
    if manifest.signing_key_id != expected_key_id {
        return Err(AppError::unprocessable(
            "WORKFLOW_PACKAGE_SIGNING_KEY_UNTRUSTED",
            "Workflow Package was signed by an untrusted key",
        ));
    }
    let signature = URL_SAFE_NO_PAD.decode(&manifest.signature).map_err(|_| {
        AppError::unprocessable(
            "WORKFLOW_PACKAGE_SIGNATURE_INVALID",
            "Workflow Package signature is not valid base64url",
        )
    })?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(AppError::internal)?;
    mac.update(manifest.content_hash.as_bytes());
    mac.verify_slice(&signature).map_err(|_| {
        AppError::unprocessable(
            "WORKFLOW_PACKAGE_SIGNATURE_INVALID",
            "Workflow Package signature verification failed",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_export_replaces_real_ids_with_explicit_placeholders() {
        let resource_id = Uuid::new_v4();
        let mut definition = json!({
            "nodes": [{
                "id": "model",
                "resourceReferences": [{
                    "resourceType": "model",
                    "resourceId": resource_id,
                    "resourceVersionId": Uuid::new_v4(),
                    "operation": "use"
                }]
            }]
        });

        let placeholders = sanitize_resource_references(&mut definition).expect("sanitize");
        assert_eq!(placeholders[0].id, "resource.model.0");
        assert_eq!(
            definition["nodes"][0]["resourceReferences"][0]["resourceId"],
            Uuid::nil().to_string()
        );
        assert_eq!(
            definition["nodes"][0]["resourceReferences"][0]["resourceVersionId"],
            Value::Null
        );

        let target = Uuid::new_v4();
        apply_resource_bindings(
            &mut definition,
            &placeholders,
            &BTreeMap::from([(
                "resource.model.0".into(),
                ResourceBindingTarget {
                    resource_id: target,
                    resource_version_id: None,
                },
            )]),
        )
        .expect("bind resource");
        assert_eq!(
            definition["nodes"][0]["resourceReferences"][0]["resourceId"],
            target.to_string()
        );
    }

    #[test]
    fn package_hash_changes_when_definition_changes() {
        let locks = vec![NodeLock {
            node_type: "set".into(),
            version: 1,
            manifest_hash: "sha256:test".into(),
        }];
        let first = package_content_hash(
            &json!({"schemaVersion":"4.0"}),
            &json!({}),
            &locks,
            &[],
            &[],
            &json!({}),
        )
        .expect("hash");
        let second = package_content_hash(
            &json!({"schemaVersion":"4.0","settings":{"timeoutMs":10}}),
            &json!({}),
            &locks,
            &[],
            &[],
            &json!({}),
        )
        .expect("hash");
        assert_ne!(first, second);
    }

    #[test]
    fn package_hash_survives_json_and_browser_number_round_trips() {
        let locks = vec![NodeLock {
            node_type: "set".into(),
            version: 1,
            manifest_hash: "sha256:test".into(),
        }];
        let definition = json!({
            "schemaVersion":"4.0",
            "nodes":[{"parameters":{"value":1.0}}]
        });
        let editor = json!({
            "nodeLayouts":[{"nodeId":"set","x":120.0,"y":180.0}],
            "viewport":{"x":0.0,"y":0.0,"zoom":1.0}
        });
        let exported_hash =
            package_content_hash(&definition, &editor, &locks, &[], &[], &json!({}))
                .expect("export hash");

        let package = WorkflowPackage {
            manifest: WorkflowPackageManifest {
                package_schema_version: "1.0".into(),
                workflow_schema_version: "4.0".into(),
                name: "Round trip".into(),
                content_hash: exported_hash.clone(),
                signature_algorithm: "hmac-sha256".into(),
                signing_key_id: "test".into(),
                signature: "test".into(),
            },
            workflow_definition: definition,
            editor_document: editor,
            node_lock: locks.clone(),
            sub_workflow_references: vec![],
            resource_binding_placeholders: vec![],
            api_binding: json!({}),
        };
        let decoded: WorkflowPackage =
            serde_json::from_str(&serde_json::to_string(&package).expect("serialize package"))
                .expect("deserialize package");
        assert_eq!(
            exported_hash,
            package_content_hash(
                &decoded.workflow_definition,
                &decoded.editor_document,
                &decoded.node_lock,
                &decoded.sub_workflow_references,
                &decoded.resource_binding_placeholders,
                &decoded.api_binding,
            )
            .expect("decoded hash")
        );

        let browser_hash = package_content_hash(
            &json!({
                "schemaVersion":"4.0",
                "nodes":[{"parameters":{"value":1}}]
            }),
            &json!({
                "nodeLayouts":[{"nodeId":"set","x":120,"y":180}],
                "viewport":{"x":0,"y":0,"zoom":1}
            }),
            &locks,
            &[],
            &[],
            &json!({}),
        )
        .expect("browser hash");
        assert_eq!(exported_hash, browser_hash);
    }

    #[test]
    fn missing_resource_binding_is_rejected() {
        let mut definition = json!({"nodes":[{"id":"code","resourceReferences":[{"resourceType":"sandbox_profile","resourceId":Uuid::nil()}]}]});
        let placeholders = sanitize_resource_references(&mut definition).expect("sanitize");
        let error = apply_resource_bindings(&mut definition, &placeholders, &BTreeMap::new())
            .expect_err("missing binding");
        assert_eq!(error.code, "RESOURCE_BINDING_REQUIRED");
    }

    #[test]
    fn package_signature_rejects_tampering_and_untrusted_keys() {
        let key = b"shared-workflow-package-key";
        let (signing_key_id, signature) =
            sign_package_hash_with_key(key, "sha256:content").expect("sign");
        let mut manifest = WorkflowPackageManifest {
            package_schema_version: "1.0".into(),
            workflow_schema_version: "4.0".into(),
            name: "Portable workflow".into(),
            content_hash: "sha256:content".into(),
            signature_algorithm: "hmac-sha256".into(),
            signing_key_id,
            signature,
        };
        verify_package_signature_with_key(key, &manifest).expect("trusted signature");

        manifest.content_hash = "sha256:tampered".into();
        assert_eq!(
            verify_package_signature_with_key(key, &manifest)
                .expect_err("tampering must fail")
                .code,
            "WORKFLOW_PACKAGE_SIGNATURE_INVALID"
        );
        manifest.content_hash = "sha256:content".into();
        assert_eq!(
            verify_package_signature_with_key(b"another-key", &manifest)
                .expect_err("unknown signer must fail")
                .code,
            "WORKFLOW_PACKAGE_SIGNING_KEY_UNTRUSTED"
        );
    }
}
