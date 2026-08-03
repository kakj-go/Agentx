$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$namespace = "agentx"

kubectl -n $namespace get deployment lightrag mem0 mem0-postgres --ignore-not-found
kubectl -n $namespace get service lightrag mem0 mem0-postgres --ignore-not-found
kubectl -n $namespace get pvc lightrag-data mem0-history-data mem0-postgres-data --ignore-not-found
