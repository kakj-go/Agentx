param(
    [string]$Tag = "dev"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$services = @(
    "platform-api",
    "trigger-gateway",
    "workflow-coordinator",
    "workflow-worker",
    "sandbox-manager",
    "trace-writer"
)

foreach ($service in $services) {
    $image = "agentx/{0}:{1}" -f $service, $Tag
    docker build --file "$root/deploy/docker/backend.Dockerfile" --build-arg "APP=$service" --tag $image $root
}

docker build --file "$root/deploy/docker/web.Dockerfile" --tag "agentx/web:$Tag" $root
