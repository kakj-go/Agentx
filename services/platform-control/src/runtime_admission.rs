use agentx_runtime_contracts::{
    AdmissionTargetV1, CommandEnvelopeV1, Plane, RuntimeAdmissionCommandV1, RuntimeGrantStateV1,
    ServiceIdentityAdmissionV1, content_hash,
};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api_error::{ApiError, ApiResult};
use crate::control_api::ControlApiState;

#[derive(Clone, Debug)]
pub(crate) struct RevokedResourceGrant {
    pub grant_id: Uuid,
    pub identity_id: Uuid,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub operation: String,
}

pub(crate) async fn emit_new_service_identity(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    identity_id: Uuid,
) -> ApiResult<()> {
    emit_identity_snapshot(tx, tenant_id, identity_id, 1, &[])
        .await
        .map(|_| ())
}

pub(crate) async fn advance_service_identity(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    identity_id: Uuid,
    revoked: &[RevokedResourceGrant],
) -> ApiResult<Vec<RuntimeAdmissionCommandV1>> {
    let changed = sqlx::query(
        "UPDATE workflow_service_identities SET version=version+1 WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(identity_id)
    .execute(&mut **tx)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::not_found("Workflow service identity"));
    }
    let epoch: u64 = sqlx::query_scalar(
        "SELECT version FROM workflow_service_identities WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(identity_id)
    .fetch_one(&mut **tx)
    .await?;
    emit_identity_snapshot(tx, tenant_id, identity_id, epoch, revoked).await
}

async fn emit_identity_snapshot(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    identity_id: Uuid,
    epoch: u64,
    revoked: &[RevokedResourceGrant],
) -> ApiResult<Vec<RuntimeAdmissionCommandV1>> {
    let identity = sqlx::query(
        "SELECT workflow_id,status FROM workflow_service_identities WHERE tenant_id=? AND id=?",
    )
    .bind(tenant_id)
    .bind(identity_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ApiError::not_found("Workflow service identity"))?;
    let workflow_id: Uuid = identity.try_get("workflow_id")?;
    let status: String = identity.try_get("status")?;
    let grants = sqlx::query("SELECT id,resource_type,resource_id,operation_key FROM resource_grants WHERE tenant_id=? AND subject_type='workflow_service_identity' AND subject_id=? ORDER BY id")
        .bind(tenant_id)
        .bind(identity_id)
        .fetch_all(&mut **tx)
        .await?;
    let grant_ids = grants
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut commands = Vec::with_capacity(grants.len() + revoked.len() + 1);

    // Publish every Grant at the new Epoch before advancing the identity. Once the
    // identity is visible, Runtime can therefore authorize the complete snapshot.
    for row in grants {
        let grant_id: Uuid = row.try_get("id")?;
        let target = AdmissionTargetV1::ResourceGrant {
            state: RuntimeGrantStateV1 {
                tenant_id,
                identity_id,
                grant_id,
                resource_kind: crate::runtime_resource_kind_from_control(
                    &row.try_get::<String, _>("resource_type")?,
                )
                .map_err(ApiError::internal)?,
                resource_id: row.try_get("resource_id")?,
                operations: [row.try_get::<String, _>("operation_key")?]
                    .into_iter()
                    .collect(),
                policy_epoch: epoch,
                enabled: true,
            },
        };
        commands.push(
            emit(
                tx,
                tenant_id,
                "ResourceGrantAdmissionChanged",
                grant_id,
                json!({
                    "identityId": identity_id,
                    "grantId": grant_id,
                    "resourceType": row.try_get::<String, _>("resource_type")?,
                    "resourceId": row.try_get::<Uuid, _>("resource_id")?,
                    "operations": [row.try_get::<String, _>("operation_key")?],
                    "enabled": true,
                    "policyEpoch": epoch,
                    "admissionEpoch": epoch,
                }),
                format!("resource-grant-admission:{grant_id}:{epoch}:active"),
                target,
            )
            .await?,
        );
    }
    for grant in revoked {
        debug_assert_eq!(grant.identity_id, identity_id);
        let target = AdmissionTargetV1::ResourceGrant {
            state: RuntimeGrantStateV1 {
                tenant_id,
                identity_id: grant.identity_id,
                grant_id: grant.grant_id,
                resource_kind: crate::runtime_resource_kind_from_control(&grant.resource_type)
                    .map_err(ApiError::internal)?,
                resource_id: grant.resource_id,
                operations: [grant.operation.clone()].into_iter().collect(),
                policy_epoch: epoch,
                enabled: false,
            },
        };
        commands.push(
            emit(
                tx,
                tenant_id,
                "ResourceGrantAdmissionChanged",
                grant.grant_id,
                json!({
                    "identityId": grant.identity_id,
                    "grantId": grant.grant_id,
                    "resourceType": grant.resource_type,
                    "resourceId": grant.resource_id,
                    "operations": [grant.operation],
                    "enabled": false,
                    "policyEpoch": epoch,
                    "admissionEpoch": epoch,
                }),
                format!(
                    "resource-grant-admission:{}:{epoch}:revoked",
                    grant.grant_id
                ),
                target,
            )
            .await?,
        );
    }
    let target = AdmissionTargetV1::ServiceIdentity {
        state: ServiceIdentityAdmissionV1 {
            tenant_id,
            workflow_id,
            identity_id,
            policy_epoch: epoch,
            status: match status.as_str() {
                "active" => agentx_runtime_contracts::AdmissionStatusV1::Active,
                "disabled" => agentx_runtime_contracts::AdmissionStatusV1::Disabled,
                _ => agentx_runtime_contracts::AdmissionStatusV1::Revoked,
            },
            capabilities: agentx_node_protocol::ALL_RUNTIME_CAPABILITIES
                .iter()
                .map(ToString::to_string)
                .collect(),
            grant_ids: grant_ids.clone(),
        },
    };
    commands.push(
        emit(
            tx,
            tenant_id,
            "ServiceIdentityAdmissionChanged",
            identity_id,
            json!({
                "workflowId": workflow_id,
                "identityId": identity_id,
                "status": status,
                "policyEpoch": epoch,
                "capabilities": agentx_node_protocol::ALL_RUNTIME_CAPABILITIES,
                "grantIds": grant_ids,
                "admissionEpoch": epoch,
            }),
            format!("service-identity-admission:{identity_id}:{epoch}"),
            target,
        )
        .await?,
    );
    Ok(commands)
}

async fn emit(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    event_type: &str,
    aggregate_id: Uuid,
    payload: Value,
    idempotency_key: String,
    target: AdmissionTargetV1,
) -> ApiResult<RuntimeAdmissionCommandV1> {
    let event_id = Uuid::now_v7();
    let request_hash = content_hash(&payload)
        .map_err(ApiError::internal)?
        .to_string();
    sqlx::query("INSERT INTO outbox(id,tenant_id,event_type,aggregate_type,aggregate_id,payload_json,status,request_hash,idempotency_key) VALUES(?,?,?,'workflow_admission',?,?,'pending',?,?)")
        .bind(event_id)
        .bind(tenant_id)
        .bind(event_type)
        .bind(aggregate_id.to_string())
        .bind(payload)
        .bind(request_hash)
        .bind(idempotency_key)
        .execute(&mut **tx)
        .await?;
    let occurred_at: OffsetDateTime =
        sqlx::query_scalar("SELECT occurred_at FROM outbox WHERE id=?")
            .bind(event_id)
            .fetch_one(&mut **tx)
            .await?;
    Ok(RuntimeAdmissionCommandV1 {
        api_version: 1,
        command: CommandEnvelopeV1 {
            schema_version: 1,
            event_id,
            source_plane: Plane::Control,
            tenant_id,
            aggregate_type: "workflow_admission".into(),
            aggregate_id: aggregate_id.to_string(),
            object_version: target_epoch(&target),
            occurred_at,
            payload: json!({}),
            content_hash: content_hash(&target).map_err(ApiError::internal)?,
            correlation_id: event_id,
            causation_id: None,
            idempotency_key: format!("{event_id}:admission"),
        },
        admission_epoch: target_epoch(&target),
        target,
    })
}

fn target_epoch(target: &AdmissionTargetV1) -> u64 {
    match target {
        AdmissionTargetV1::ResourceGrant { state } => state.policy_epoch,
        AdmissionTargetV1::ServiceIdentity { state } => state.policy_epoch,
        _ => unreachable!("workflow identity admission emits only Grant and Identity targets"),
    }
}

pub(crate) async fn publish_barrier(
    state: &ControlApiState,
    commands: Vec<RuntimeAdmissionCommandV1>,
) -> ApiResult<()> {
    for command in commands {
        let receipt = crate::governance_api::post_runtime_admission(state, &command).await?;
        if !receipt.applied {
            return Err(ApiError::conflict(
                "RUNTIME_ADMISSION_REJECTED",
                "Runtime rejected the authorization update",
            ));
        }
        sqlx::query("UPDATE outbox SET status='published',published_at=COALESCE(published_at,UTC_TIMESTAMP(6)),last_error=NULL WHERE id=? AND status IN ('pending','failed')")
            .bind(command.command.event_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}
