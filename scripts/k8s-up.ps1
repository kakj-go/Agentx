$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

kubectl -n agentx delete job platform-api-migrate --ignore-not-found
kubectl -n agentx delete job trace-writer-migrate --ignore-not-found
kubectl apply -k "$root/deploy/k8s/overlays/local"
kubectl -n agentx rollout status statefulset/mysql --timeout=420s
kubectl -n agentx wait --for=condition=complete job/platform-api-migrate --timeout=300s
kubectl -n agentx rollout status statefulset/redis --timeout=420s
kubectl -n agentx rollout status statefulset/clickhouse --timeout=420s
kubectl -n agentx wait --for=condition=complete job/trace-writer-migrate --timeout=300s
kubectl -n agentx rollout status statefulset/minio --timeout=420s
$deployments = @(
    "web",
    "echo-mcp",
    "echo-node",
    "platform-api",
    "trigger-gateway",
    "workflow-coordinator",
    "workflow-worker",
    "sandbox-manager",
    "trace-writer"
)

foreach ($deployment in $deployments) {
    kubectl -n agentx rollout restart "deployment/$deployment"
}

foreach ($deployment in $deployments) {
    kubectl -n agentx rollout status "deployment/$deployment" --timeout=180s
}
