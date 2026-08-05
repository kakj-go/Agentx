param([switch]$Force)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$namespace = "agentx"
$claim = "data-mysql-0"

$context = kubectl config current-context
if (-not $Force) {
    $answer = Read-Host "Delete PVC $namespace/$claim on context '$context'? Type RESET to continue"
    if ($answer -cne "RESET") {
        Write-Output "Reset cancelled."
        exit 0
    }
}

$resolved = kubectl -n $namespace get pvc $claim -o jsonpath='{.metadata.name}' 2>$null
if ($resolved -and $resolved -cne $claim) {
    throw "Resolved PVC does not match the expected development claim."
}

kubectl -n $namespace scale statefulset/mysql --replicas=0
kubectl -n $namespace wait --for=delete pod/mysql-0 --timeout=180s
kubectl -n $namespace delete pvc $claim --ignore-not-found
kubectl -n $namespace delete job platform-api-migrate --ignore-not-found
kubectl -n $namespace apply -k "$root/deploy/k8s/infrastructure/mysql"
kubectl -n $namespace rollout status statefulset/mysql --timeout=420s
kubectl -n $namespace wait --for=condition=complete job/platform-api-migrate --timeout=300s

Write-Output "Agentx development MySQL was recreated from all current migrations."
