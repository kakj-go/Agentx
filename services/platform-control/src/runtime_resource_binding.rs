use std::collections::BTreeSet;

use agentx_bundle_builder::composite_ir_object_id;
use agentx_domain::ResourceVersionSnapshot;
use agentx_runtime_contracts::{
    ContentHash, RuntimeResourceBindingV1, RuntimeResourceConfigurationV1, RuntimeResourceKindV1,
    VaultSecretReferenceV1,
};
use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

pub(crate) fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<RuntimeResourceBindingV1> {
    let resource_type: String = row.try_get("resource_type")?;
    let resource_id: Uuid = row.try_get("resource_id")?;
    let version_id: Option<Uuid> = row.try_get("resource_version_id")?;
    let snapshot: Value = row.try_get("snapshot_json")?;
    let content_hash = ContentHash::parse(row.try_get::<String, _>("snapshot_hash")?)?;
    from_parts(
        &resource_type,
        resource_id,
        version_id,
        snapshot,
        content_hash,
    )
}

pub(crate) fn from_snapshot(value: &ResourceVersionSnapshot) -> Result<RuntimeResourceBindingV1> {
    from_parts(
        value.reference.resource_type.as_str(),
        value.reference.resource_id,
        value.reference.resource_version_id,
        value.snapshot.clone(),
        ContentHash::parse(&value.snapshot_hash)?,
    )
}

fn from_parts(
    resource_type: &str,
    resource_id: Uuid,
    version_id: Option<Uuid>,
    snapshot: Value,
    content_hash: ContentHash,
) -> Result<RuntimeResourceBindingV1> {
    let resource_version = version_id
        .map(|id| id.to_string())
        .or_else(|| json_string(&snapshot, "resourceVersion"))
        .unwrap_or_else(|| "fixed".into());
    let state_epoch = snapshot
        .get("resourceVersion")
        .or_else(|| snapshot.get("versionNumber"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let (resource_kind, configuration, object_ids) = match resource_type {
        "model" => (
            RuntimeResourceKindV1::Model,
            RuntimeResourceConfigurationV1::Model {
                provider: required_json_string(&snapshot, "providerType")?,
                endpoint: required_json_string(&snapshot, "endpoint")?,
                model: required_json_string(&snapshot, "modelName")?,
                price_version: optional_json_scalar_string(snapshot.pointer("/price/versionId"))?
                    .unwrap_or_else(|| "unpriced".into()),
                credential: optional_vault_reference(&snapshot)?,
            },
            vec![],
        ),
        "mcp_tool" | "mcp_server" => {
            let schema_hash = snapshot
                .get("schemaHash")
                .or_else(|| snapshot.get("configurationHash"))
                .and_then(Value::as_str)
                .context("MCP Runtime binding requires schemaHash")?;
            (
                RuntimeResourceKindV1::Mcp,
                RuntimeResourceConfigurationV1::Mcp {
                    endpoint: required_json_string(&snapshot, "endpoint")?,
                    tool_name: json_string(&snapshot, "toolName")
                        .unwrap_or_else(|| "__server__".into()),
                    tool_version: version_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| resource_version.clone()),
                    input_schema_hash: ContentHash::parse(schema_hash)?,
                    credential: optional_vault_reference(&snapshot)?,
                },
                vec![],
            )
        }
        "rag" => (
            RuntimeResourceKindV1::Rag,
            RuntimeResourceConfigurationV1::Rag {
                endpoint: required_json_string(&snapshot, "endpoint")?,
                namespace: required_json_string(&snapshot, "externalResourceId")?,
                index_version: resource_version.clone(),
                credential: optional_vault_reference(&snapshot)?,
            },
            vec![],
        ),
        "memory" => (
            RuntimeResourceKindV1::Memory,
            RuntimeResourceConfigurationV1::Memory {
                endpoint: required_json_string(&snapshot, "endpoint")?,
                namespace: required_json_string(&snapshot, "externalNamespace")?,
                memory_version: resource_version.clone(),
                credential: optional_vault_reference(&snapshot)?,
            },
            vec![],
        ),
        "skill" => {
            let entrypoint =
                version_id.context("Skill Runtime binding requires a fixed version")?;
            let dependency_object_ids = uuid_array(&snapshot, "dependencyObjectIds")?;
            let mut object_ids = vec![entrypoint];
            object_ids.extend(dependency_object_ids.iter().copied());
            (
                RuntimeResourceKindV1::Skill,
                RuntimeResourceConfigurationV1::Skill {
                    entrypoint_object_id: entrypoint,
                    dependency_object_ids,
                },
                object_ids,
            )
        }
        "credential" => (
            RuntimeResourceKindV1::Credential,
            RuntimeResourceConfigurationV1::Credential {
                secret: required_vault_reference(&snapshot)?,
                allowed_operations: BTreeSet::from(["use".into()]),
            },
            vec![],
        ),
        "sandbox_profile" => (
            RuntimeResourceKindV1::SandboxProfile,
            RuntimeResourceConfigurationV1::SandboxProfile {
                provider: json_string(&snapshot, "provider")
                    .unwrap_or_else(|| "opensandbox".into()),
                image: required_json_string(&snapshot, "imageDigest")?,
                cpu_millis: required_json_u32(&snapshot, "cpuMillis")?,
                memory_bytes: required_json_u64(&snapshot, "memoryBytes")?,
                disk_bytes: required_json_u64(&snapshot, "diskBytes")?,
                pid_limit: required_json_u32(&snapshot, "pidsLimit")?,
                network_policy: snapshot
                    .pointer("/networkPolicy/defaultAction")
                    .and_then(Value::as_str)
                    .context("Sandbox binding requires networkPolicy.defaultAction")?
                    .to_owned(),
                maximum_ttl_seconds: required_json_u32(&snapshot, "timeoutSeconds")?,
            },
            vec![],
        ),
        "workflow" => {
            let workflow_version_id = version_id
                .or_else(|| json_uuid(&snapshot, "workflowVersionId"))
                .context("Composite Runtime binding requires a fixed Workflow Version")?;
            (
                RuntimeResourceKindV1::Composite,
                RuntimeResourceConfigurationV1::Composite {
                    workflow_version_id,
                    definition_object_id: workflow_version_id,
                    ir_object_id: composite_ir_object_id(workflow_version_id),
                },
                vec![
                    workflow_version_id,
                    composite_ir_object_id(workflow_version_id),
                ],
            )
        }
        other => anyhow::bail!("unsupported Runtime resource type {other}"),
    };
    Ok(RuntimeResourceBindingV1 {
        resource_kind,
        resource_id,
        resource_version,
        state_epoch,
        content_hash,
        configuration,
        object_ids,
    })
}

fn optional_vault_reference(snapshot: &Value) -> Result<Option<VaultSecretReferenceV1>> {
    let value = snapshot
        .get("vaultSecretRef")
        .or_else(|| snapshot.get("secretRef"));
    value
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn required_vault_reference(snapshot: &Value) -> Result<VaultSecretReferenceV1> {
    optional_vault_reference(snapshot)?
        .context("Credential Runtime binding requires a versioned Vault secret reference")
}

pub(crate) fn required_json_string(value: &Value, key: &str) -> Result<String> {
    json_string(value, key).with_context(|| format!("Runtime binding requires {key}"))
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn json_scalar_string(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => anyhow::bail!("Runtime binding version must be a string or number"),
    }
}

fn optional_json_scalar_string(value: Option<&Value>) -> Result<Option<String>> {
    value
        .filter(|value| !value.is_null())
        .map(json_scalar_string)
        .transpose()
}

pub(crate) fn json_uuid(value: &Value, key: &str) -> Option<Uuid> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

pub(crate) fn required_json_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("Runtime binding requires numeric {key}"))
}

fn required_json_u32(value: &Value, key: &str) -> Result<u32> {
    required_json_u64(value, key)?
        .try_into()
        .with_context(|| format!("Runtime binding {key} exceeds u32"))
}

fn uuid_array(value: &Value, key: &str) -> Result<Vec<Uuid>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            let value = value
                .as_str()
                .with_context(|| format!("Runtime binding {key} must contain UUID strings"))?;
            Uuid::parse_str(value).map_err(Into::into)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::optional_json_scalar_string;
    use serde_json::json;

    #[test]
    fn null_optional_versions_are_absent() {
        let snapshot = json!({"price":{"versionId":null}});
        assert_eq!(
            optional_json_scalar_string(snapshot.pointer("/price/versionId")).unwrap(),
            None
        );
        let snapshot = json!({"price":{"versionId":7}});
        assert_eq!(
            optional_json_scalar_string(snapshot.pointer("/price/versionId")).unwrap(),
            Some("7".into())
        );
    }
}
