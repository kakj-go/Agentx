$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

kubectl apply -k "$root/deploy/k8s/overlays/local"
kubectl -n agentx rollout status statefulset/mysql --timeout=420s
kubectl -n agentx rollout status statefulset/redis --timeout=420s
kubectl -n agentx rollout status statefulset/clickhouse --timeout=420s
kubectl -n agentx rollout status statefulset/minio --timeout=420s
$deployments = @(
    "web",
    "platform-api",
    "trigger-gateway",
    "workflow-coordinator",
    "workflow-worker",
    "sandbox-manager",
    "trace-writer"
)

foreach ($deployment in $deployments) {
    kubectl -n agentx rollout status "deployment/$deployment" --timeout=180s
}
