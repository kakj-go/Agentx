use agentx_runtime::StringConversionRecord;
use agentx_runtime_contracts::{TraceContentKindV1, TraceEventKindV1, TraceSpanKindV1};
use sqlx::{MySql, Transaction};
use uuid::Uuid;

pub(super) async fn enqueue_node_records(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    node_execution_id: Uuid,
    attempt_id: Uuid,
    records: &[StringConversionRecord],
) {
    if records.is_empty() {
        return;
    }
    let mut trace = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        attempt_id,
        Some((node_execution_id, TraceSpanKindV1::Node)),
        TraceSpanKindV1::Attempt,
        "Attempt",
        TraceEventKindV1::Updated,
        "dynamic_value.string_converted",
        "succeeded",
    );
    trace.node_execution_id = Some(node_execution_id);
    trace.attempt_id = Some(attempt_id);
    trace.attributes = serde_json::json!({
        "diagnostic":"string_conversion",
        "conversionCount":records.len(),
    });
    trace.content_kind = Some(TraceContentKindV1::ConversionRecord);
    trace.content_preview = crate::trace_delivery::bounded_preview(&serde_json::json!({
        "records":records,
    }));
    crate::trace_delivery::enqueue_best_effort(tx, trace).await;
}

pub(crate) async fn enqueue_end_records(
    tx: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    execution_id: Uuid,
    records: &[StringConversionRecord],
) {
    if records.is_empty() {
        return;
    }
    let mut trace = crate::trace_delivery::TraceDraft::span(
        tenant_id,
        execution_id,
        crate::trace_delivery::boundary_entity_id(execution_id, "end"),
        Some((execution_id, TraceSpanKindV1::Execution)),
        TraceSpanKindV1::Boundary,
        "End",
        TraceEventKindV1::Updated,
        "end.string_converted",
        "succeeded",
    );
    trace.attributes = serde_json::json!({
        "diagnostic":"string_conversion",
        "conversionCount":records.len(),
    });
    trace.content_kind = Some(TraceContentKindV1::ConversionRecord);
    trace.content_preview = crate::trace_delivery::bounded_preview(&serde_json::json!({
        "records":records,
    }));
    crate::trace_delivery::enqueue_best_effort(tx, trace).await;
}
