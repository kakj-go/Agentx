param(
    [ValidateSet("all", "lightrag", "mem0")]
    [string]$Addon = "all",
    [switch]$RebuildMem0
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$namespace = "agentx"
$mem0Image = "agentx/mem0-server:v2.0.15"

function Import-LocalImage([string]$Image) {
    $node = docker ps --filter "name=^/desktop-control-plane$" --format "{{.Names}}" | Select-Object -First 1
    if (-not $node) {
        throw "Automatic local image import currently requires Docker Desktop Kubernetes."
    }
    $containerdImage = "docker.io/$Image"
    docker exec $node sh -c "ctr --namespace k8s.io images remove '$containerdImage' >/dev/null 2>&1 || true"
    docker save $Image | docker exec -i $node ctr --namespace k8s.io images import -
}

kubectl create namespace $namespace --dry-run=client -o yaml | kubectl apply -f -

if ($Addon -in @("all", "lightrag")) {
    kubectl apply -k "$root/deploy/k8s/addons/lightrag"
    kubectl -n $namespace rollout status deployment/lightrag --timeout=600s
}

if ($Addon -in @("all", "mem0")) {
    $exists = docker image inspect $mem0Image 2>$null
    if ($RebuildMem0 -or -not $exists) {
        docker build --file server/dev.Dockerfile --tag $mem0Image "https://github.com/mem0ai/mem0.git#v2.0.15"
    }
    Import-LocalImage $mem0Image
    kubectl apply -k "$root/deploy/k8s/addons/mem0"
    kubectl -n $namespace rollout status deployment/mem0-postgres --timeout=420s
    kubectl -n $namespace rollout status deployment/mem0 --timeout=600s
}

& "$PSScriptRoot/k8s-addons-status.ps1"
