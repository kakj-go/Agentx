param(
    [string]$Tag = "dev",
    [string[]]$Services = @(
        "platform-api",
        "echo-mcp",
        "echo-node",
        "trigger-gateway",
        "workflow-coordinator",
        "workflow-worker",
        "sandbox-manager",
        "trace-writer"
    ),
    [switch]$SkipWeb
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$kubernetesNode = docker ps --filter "name=^/desktop-control-plane$" --format "{{.Names}}" | Select-Object -First 1
$imageLoaderPod = "agentx-image-loader"
$imageLoaderReady = $false

function Initialize-KubernetesImageLoader {
    if ($script:imageLoaderReady -or $kubernetesNode) {
        return
    }

    kubectl create namespace agentx --dry-run=client -o yaml | kubectl apply -f -
    kubectl -n agentx delete pod $imageLoaderPod --ignore-not-found --wait=true
    $manifest = @'
apiVersion: v1
kind: Pod
metadata:
  name: agentx-image-loader
  namespace: agentx
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
'@
    $manifest | kubectl apply -f -
    kubectl -n agentx wait --for=condition=Ready "pod/$imageLoaderPod" --timeout=180s
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
    kubectl -n agentx exec $imageLoaderPod -- sh -c "ctr --address /run/containerd/containerd.sock --namespace k8s.io images remove '$containerdImage' >/dev/null 2>&1 || true"
    $archive = Join-Path ([System.IO.Path]::GetTempPath()) ("agentx-image-{0}-{1}.tar" -f $PID, [Guid]::NewGuid().ToString("N"))
    try {
        docker save --output $archive $Image
        Push-Location ([System.IO.Path]::GetDirectoryName($archive))
        try {
            kubectl -n agentx cp ([System.IO.Path]::GetFileName($archive)) "${imageLoaderPod}:/tmp/agentx-image.tar"
        }
        finally {
            Pop-Location
        }
        kubectl -n agentx exec $imageLoaderPod -- ctr --address /run/containerd/containerd.sock --namespace k8s.io images import /tmp/agentx-image.tar
    }
    finally {
        Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
    }
}
try {
    foreach ($service in $Services) {
        $image = "agentx/{0}:{1}" -f $service, $Tag
        docker build --file "$root/deploy/docker/backend.Dockerfile" --build-arg "APP=$service" --tag $image $root
        Import-LocalKubernetesImage $image
    }

    if (-not $SkipWeb) {
        $webImage = "agentx/web:$Tag"
        docker build --file "$root/deploy/docker/web.Dockerfile" --tag $webImage $root
        Import-LocalKubernetesImage $webImage
    }
}
finally {
    if ($imageLoaderReady) {
        kubectl -n agentx delete pod $imageLoaderPod --ignore-not-found --wait=true
    }
}
