param(
    [ValidateSet("all", "lightrag", "mem0")]
    [string]$Addon = "all",
    [switch]$DeleteData
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$namespace = "agentx"

if ($Addon -in @("all", "lightrag")) {
    kubectl -n $namespace delete deployment/lightrag service/lightrag configmap/agentx-lightrag-config secret/agentx-lightrag-secrets --ignore-not-found
    if ($DeleteData) {
        kubectl -n $namespace delete pvc/lightrag-data --ignore-not-found
    }
}

if ($Addon -in @("all", "mem0")) {
    kubectl -n $namespace delete deployment/mem0 deployment/mem0-postgres service/mem0 service/mem0-postgres configmap/agentx-mem0-config configmap/agentx-mem0-postgres-init secret/agentx-mem0-secrets --ignore-not-found
    if ($DeleteData) {
        kubectl -n $namespace delete pvc/mem0-history-data pvc/mem0-postgres-data --ignore-not-found
    }
}
