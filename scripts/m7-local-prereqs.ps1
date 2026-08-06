param(
    [string]$RunId = ([DateTimeOffset]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")),
    [string]$OutputDirectory,
    [string]$ToolDirectory,
    [string]$KubectlVersion = "1.36.1",
    [string]$CosignVersion = "2.5.3",
    [string]$SyftVersion = "1.29.0",
    [switch]$SkipToolInstall
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$output = if ($OutputDirectory) {
    if ([IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory } else { Join-Path $root $OutputDirectory }
} else { Join-Path $root "artifacts/m7/local/$RunId" }
$tools = if ($ToolDirectory) {
    if ([IO.Path]::IsPathRooted($ToolDirectory)) { $ToolDirectory } else { Join-Path $root $ToolDirectory }
} else { Join-Path $root ".local/m7-tools" }
New-Item -ItemType Directory -Path $output, $tools -Force | Out-Null

function Require-Command([string]$Name) {
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) { throw "$Name is required." }
    $command.Source
}

function Get-RemoteText([string]$Uri) {
    $content = (Invoke-WebRequest -UseBasicParsing -Uri $Uri).Content
    if ($content -is [byte[]]) { return [Text.Encoding]::UTF8.GetString($content).Trim() }
    ([string]$content).Trim()
}

function Assert-Checksum([string]$Path, [string]$Expected) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -ne $Expected.ToLowerInvariant()) { throw "SHA-256 mismatch for $Path." }
    $actual
}

function Find-Checksum([string]$Content, [string]$FileName) {
    foreach ($line in $Content -split "`r?`n") {
        if ($line -match '^([a-fA-F0-9]{64})\s+\*?(.+)$' -and $Matches[2].Trim() -eq $FileName) { return $Matches[1].ToLowerInvariant() }
    }
    throw "Checksum file does not contain $FileName."
}

function Install-Cosign {
    $name = "cosign-windows-amd64.exe"
    $target = Join-Path $tools "cosign.exe"
    $base = "https://github.com/sigstore/cosign/releases/download/v$CosignVersion"
    $expected = Find-Checksum (Get-RemoteText "$base/cosign_checksums.txt") $name
    if (-not (Test-Path -LiteralPath $target) -or (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant() -ne $expected) {
        if ($SkipToolInstall) { throw "Verified cosign $CosignVersion is not installed in $tools." }
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$name" -OutFile $target
    }
    Assert-Checksum $target $expected | Out-Null
    $target
}

function Install-Syft {
    $archiveName = "syft_${SyftVersion}_windows_amd64.zip"
    $target = Join-Path $tools "syft.exe"
    $base = "https://github.com/anchore/syft/releases/download/v$SyftVersion"
    $expected = Find-Checksum (Get-RemoteText "$base/syft_${SyftVersion}_checksums.txt") $archiveName
    $archive = Join-Path $tools $archiveName
    $marker = "$target.sha256"
    $mustInstall = -not (Test-Path -LiteralPath $target)
    if (-not $mustInstall -and -not (Test-Path -LiteralPath $marker)) { $mustInstall = $true }
    if (-not $mustInstall -and (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant() -ne (Get-Content -Raw -LiteralPath $marker).Trim().ToLowerInvariant()) { $mustInstall = $true }
    if ($mustInstall) {
        if ($SkipToolInstall) { throw "Verified syft $SyftVersion is not installed in $tools." }
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$archiveName" -OutFile $archive
        Assert-Checksum $archive $expected | Out-Null
        $extract = Join-Path $tools "syft-$SyftVersion"
        if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
        Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
        Copy-Item -LiteralPath (Join-Path $extract "syft.exe") -Destination $target -Force
        (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant() | Set-Content -LiteralPath $marker -Encoding ascii
        Remove-Item -LiteralPath $extract -Recurse -Force
        Remove-Item -LiteralPath $archive -Force
    }
    $target
}

function Install-Kubectl {
    $target = Join-Path $tools "kubectl.exe"
    $base = "https://dl.k8s.io/release/v$KubectlVersion/bin/windows/amd64/kubectl.exe"
    $expected = (Get-RemoteText "$base.sha256").Split(' ')[0].ToLowerInvariant()
    if (-not (Test-Path -LiteralPath $target) -or (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant() -ne $expected) {
        if ($SkipToolInstall) { throw "Verified kubectl $KubectlVersion is not installed in $tools." }
        Invoke-WebRequest -UseBasicParsing -Uri $base -OutFile $target
    }
    Assert-Checksum $target $expected | Out-Null
    $target
}

if (-not $IsWindows) { throw "This local acceptance harness currently targets Docker Desktop on Windows." }
if ($PSVersionTable.PSVersion.Major -lt 7) { throw "PowerShell 7 or newer is required." }
$dockerPath = Require-Command "docker"
$gitPath = Require-Command "git"
docker info | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Docker Engine is unavailable." }

$kubectlPath = Install-Kubectl
$cosignPath = Install-Cosign
$syftPath = Install-Syft
$env:PATH = "$tools$([IO.Path]::PathSeparator)$env:PATH"

$kubernetes = kubectl version -o json | ConvertFrom-Json -Depth 20
if ($LASTEXITCODE -ne 0) { throw "Kubernetes is unavailable." }
$client = [Version]($kubernetes.clientVersion.gitVersion.TrimStart('v').Split('-')[0])
$server = [Version]($kubernetes.serverVersion.gitVersion.TrimStart('v').Split('-')[0])
if ($client.Major -ne $server.Major -or [Math]::Abs($client.Minor - $server.Minor) -gt 1) {
    throw "kubectl $client is more than one minor away from Kubernetes $server."
}
if ($client.ToString(3) -ne ([Version]$KubectlVersion).ToString(3)) { throw "Expected kubectl $KubectlVersion but resolved $client." }
$context = ([string](kubectl config current-context)).Trim()
if ($context -notmatch 'docker-desktop') { throw "Current Kubernetes context '$context' is not Docker Desktop." }
$nodes = @(kubectl get nodes -o json | ConvertFrom-Json -Depth 30).items
if ($nodes.Count -ne 1) { throw "Local acceptance requires exactly one Docker Desktop Kubernetes node." }

$cosignVersionOutput = (& $cosignPath version 2>&1 | Out-String)
if ($cosignVersionOutput -notmatch [regex]::Escape($CosignVersion)) { throw "Resolved cosign does not report version $CosignVersion." }
$syftVersionOutput = (& $syftPath version 2>&1 | Out-String)
if ($syftVersionOutput -notmatch [regex]::Escape($SyftVersion)) { throw "Resolved syft does not report version $SyftVersion." }

$toolchain = [ordered]@{
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    tools = @(
        [ordered]@{ name = "docker"; version = ((docker version --format '{{.Server.Version}}').Trim()); path = $dockerPath; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $dockerPath).Hash.ToLowerInvariant() },
        [ordered]@{ name = "git"; version = ((git --version) -replace '^git version\s+', '').Trim(); path = $gitPath; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $gitPath).Hash.ToLowerInvariant() },
        [ordered]@{ name = "kubectl"; version = $client.ToString(3); path = $kubectlPath; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $kubectlPath).Hash.ToLowerInvariant() },
        [ordered]@{ name = "cosign"; version = $CosignVersion; path = $cosignPath; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $cosignPath).Hash.ToLowerInvariant() },
        [ordered]@{ name = "syft"; version = $SyftVersion; path = $syftPath; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $syftPath).Hash.ToLowerInvariant() }
    )
}
$toolchainPath = Join-Path $output "toolchain.json"
$toolchain | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $toolchainPath -Encoding utf8NoBOM
$prereq = [ordered]@{
    status = "passed"
    runId = $RunId
    generatedAt = [DateTimeOffset]::UtcNow.ToString("O")
    environment = "local-docker-desktop"
    isolationLevel = "standard"
    containerRuntime = [string]$nodes[0].status.nodeInfo.containerRuntimeVersion
    kubernetes = [ordered]@{ context = $context; clientVersion = $client.ToString(3); serverVersion = $server.ToString(3); nodeCount = $nodes.Count }
    toolchain = [IO.Path]::GetRelativePath($root, $toolchainPath).Replace('\', '/')
}
$prereqPath = Join-Path $output "prereq.json"
$prereq | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $prereqPath -Encoding utf8NoBOM
Write-Output $prereqPath
