param(
    [string]$Tag = "dev",
    [string]$Namespace = "agentx",
    [string[]]$Services = @(
        "platform-control",
        "runtime-gateway",
        "workflow-runtime",
        "workflow-worker",
        "sandbox-manager",
        "agentx-egress-gateway",
        "observability",
        "web-console"
    ),
    [switch]$SkipWeb,
    [switch]$BuildMem0
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$kubernetesNode = docker ps --filter "name=^/desktop-control-plane$" --format "{{.Names}}" | Select-Object -First 1
$imageLoaderPod = "agentx-image-loader"
$imageLoaderReady = $false

function Invoke-DockerBuild([string[]]$DockerArguments) {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            & docker build @DockerArguments
            return
        }
        catch {
            if ($attempt -eq 3) {
                throw
            }
            Write-Warning "Docker build attempt $attempt failed; retrying after a transient backoff."
            Start-Sleep -Seconds (2 * $attempt)
        }
    }
}

function Initialize-KubernetesImageLoader {
    if ($script:imageLoaderReady -or $kubernetesNode) {
        return
    }

    $existingNamespace = kubectl get namespace $Namespace --ignore-not-found -o name
    if (-not $existingNamespace) {
        kubectl create namespace $Namespace | Out-Null
    }
    kubectl -n $Namespace delete pod $imageLoaderPod --ignore-not-found --wait=true
    $manifest = @"
apiVersion: v1
kind: Pod
metadata:
  name: agentx-image-loader
  namespace: $Namespace
spec:
  restartPolicy: Never
  containers:
    - name: agentx-image-loader
      image: ghcr.io/containerd/nerdctl:v2.3.5
      command: ["sleep", "3600"]
      securityContext:
        privileged: true
      volumeMounts:
        - name: containerd-socket
          mountPath: /run/containerd/containerd.sock
  volumes:
    - name: containerd-socket
      hostPath:
        path: /run/containerd/containerd.sock
        type: Socket
"@
    $manifest | kubectl apply -f -
    kubectl -n $Namespace wait --for=condition=Ready "pod/$imageLoaderPod" --timeout=180s
    $script:imageLoaderReady = $true
}

function Import-LocalKubernetesImage([string]$Image) {
    $containerdImage = if ($Image.Contains("/")) { "docker.io/$Image" } else { $Image }
    if ($kubernetesNode) {
        docker exec $kubernetesNode sh -c "ctr --namespace k8s.io images remove '$containerdImage' >/dev/null 2>&1 || true"
        docker save $Image | docker exec -i $kubernetesNode ctr --namespace k8s.io images import -
        return
    }

    Initialize-KubernetesImageLoader
    kubectl -n $Namespace exec $imageLoaderPod -- sh -c "ctr --address /run/containerd/containerd.sock --namespace k8s.io images remove '$containerdImage' >/dev/null 2>&1 || true"
    $archive = Join-Path ([System.IO.Path]::GetTempPath()) ("agentx-image-{0}-{1}.tar" -f $PID, [Guid]::NewGuid().ToString("N"))
    try {
        docker save --output $archive $Image
        Push-Location ([System.IO.Path]::GetDirectoryName($archive))
        try {
            kubectl -n $Namespace cp ([System.IO.Path]::GetFileName($archive)) "${imageLoaderPod}:/tmp/agentx-image.tar"
        }
        finally {
            Pop-Location
        }
        kubectl -n $Namespace exec $imageLoaderPod -- ctr --address /run/containerd/containerd.sock --namespace k8s.io images import /tmp/agentx-image.tar
    }
    finally {
        Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    }
}
try {
    foreach ($service in $Services) {
        if ($service -eq "web-console") { continue }
        $image = "agentx/{0}:{1}" -f $service, $Tag
        if ($service -eq "lightrag") {
            Invoke-DockerBuild -DockerArguments @(
                "--file", "$root/deploy/docker/lightrag.Dockerfile",
                "--tag", $image,
                $root
            )
            Import-LocalKubernetesImage $image
            continue
        }
        $application = if ($service -eq "observability") {
            "agentx-observability"
        } elseif ($service -eq "agentx-egress-smoke") {
            "egress-smoke"
        } else {
            $service
        }
        $dockerArguments = @("--file", "$root/deploy/docker/backend.Dockerfile", "--build-arg", "APP=$application")
        if ($service -eq "observability") {
            $dockerArguments += @("--build-arg", "CARGO_PACKAGE=agentx-observability")
        }
        if ($service -in @("workflow-worker", "sandbox-manager", "agentx-egress-smoke", "v2-04-fixture")) {
            $cargoPackage = "agentx-v2-runtime"
            if ($service -eq "v2-04-fixture") { $cargoPackage = "platform-control" }
            $dockerArguments += @("--build-arg", "CARGO_PACKAGE=$cargoPackage")
        }
        if ($service -eq "agentx-egress-gateway") {
            $dockerArguments += @("--build-arg", "CARGO_PACKAGE=agentx-egress-gateway")
        }
        $dockerArguments += @("--tag", $image, $root)
        Invoke-DockerBuild -DockerArguments $dockerArguments
        Import-LocalKubernetesImage $image
    }

    if (-not $SkipWeb -and $Services -contains "web-console") {
        $webImage = "agentx/web-console:$Tag"
        $webArguments = @("--file", "$root/deploy/docker/web.Dockerfile")
        $webArguments += @("--build-arg", "NGINX_CONFIG=deploy/docker/nginx-v2.conf")
        $webArguments += @("--tag", $webImage, $root)
        Invoke-DockerBuild -DockerArguments $webArguments
        Import-LocalKubernetesImage $webImage
    }

    if ($BuildMem0) {
        $mem0Image = "agentx/mem0-server:v2.0.15"
        Invoke-DockerBuild -DockerArguments @("--file", "server/dev.Dockerfile", "--tag", $mem0Image, "https://github.com/mem0ai/mem0.git#v2.0.15")
        Import-LocalKubernetesImage $mem0Image
    }
}
finally {
    if ($imageLoaderReady) {
        kubectl -n $Namespace delete pod $imageLoaderPod --ignore-not-found --wait=true
    }
}
