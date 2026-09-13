use agentx_runtime_contracts::{PluginDesignOperationRequestV1, PluginDesignOperationResponseV1};
use axum::{Json, extract::State, http::HeaderMap};

use crate::{
    RuntimeState,
    error::{RuntimeError, RuntimeResult},
};

pub async fn execute(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Json(request): Json<PluginDesignOperationRequestV1>,
) -> RuntimeResult<Json<PluginDesignOperationResponseV1>> {
    state.trust.publisher(&headers, "runtime.plugins.design")?;
    if !matches!(
        request.method.as_str(),
        "node.resolveDefinition" | "node.invokeProvider"
    ) {
        return Err(RuntimeError::InvalidRequest(
            "PLUGIN_DESIGN_METHOD_INVALID",
            "Plugin design method is unsupported".into(),
        ));
    }
    if request.plugin.package_id.trim().is_empty()
        || request.plugin.runtime_source.trim().is_empty()
        || !request.plugin.bundle_digest.starts_with("sha256:")
    {
        return Err(RuntimeError::InvalidRequest(
            "PLUGIN_BINDING_INVALID",
            "Plugin design operation requires an immutable binding".into(),
        ));
    }
    let worker =
        crate::worker_runtime::RuntimeWorker::new(state.pool.clone(), state.objects.clone())
            .map_err(RuntimeError::Internal)?;
    let result = crate::worker_runtime::plugin::invoke_design_operation(
        &worker,
        request.tenant_id,
        request.operation_id,
        &request.resources,
        &request.plugin,
        &request.method,
        request.parameters,
    )
    .await
    .map_err(|message| RuntimeError::InvalidRequest("PLUGIN_DESIGN_OPERATION_FAILED", message))?;
    Ok(Json(PluginDesignOperationResponseV1 {
        protocol_version: 1,
        operation_id: request.operation_id,
        result,
    }))
}
