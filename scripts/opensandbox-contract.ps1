param(
    [string]$OpenSandboxEndpoint = "http://127.0.0.1:18080",
    [string]$OpenSandboxApiKey = "agentx-local-opensandbox-key",
    [string]$OpenSandboxImage = "opensandbox/code-interpreter:latest",
    [string]$SourceRoot,
    [string]$ResultsPath,
    [switch]$SkipRustTests
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot
$commit = "e95681e791b33b3893033940cbeaa5ab192bf21b"
$specs = @{
    "specs/sandbox-lifecycle.yml" = "da84de4d80cdad83c47d771135645fbeb8d7477bc8f908cc4b374397010ed6d2"
    "specs/execd-api.yaml" = "0f03effe1dc5f340d13e39d6e8c815b5bdebb880183db05bea7d592696d5f5e0"
}
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) "agentx-opensandbox-oracle-$([Guid]::NewGuid().ToString('N'))"
$clonedSource = $false

function Invoke-Native {
    param([string]$Name, [scriptblock]$Command)
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE."
    }
}

function Assert-Hash([string]$Path, [string]$Expected) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -cne $Expected) {
        throw "OpenSandbox spec drift detected for $Path. Expected $Expected, got $actual."
    }
}

New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
try {
    if (-not $SourceRoot) {
        $SourceRoot = Join-Path $root ".local/opensandbox-source"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $SourceRoot ".git"))) {
        $SourceRoot = Join-Path $temporaryRoot "opensandbox-source"
        Invoke-Native "OpenSandbox source clone" { git clone --filter=blob:none --no-checkout https://github.com/opensandbox-group/OpenSandbox.git $SourceRoot }
        Invoke-Native "OpenSandbox fixed commit checkout" { git -C $SourceRoot fetch --depth 1 origin $commit; git -C $SourceRoot checkout --detach $commit }
        $clonedSource = $true
    }
    $actualCommit = (& git -C $SourceRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualCommit -cne $commit) {
        throw "OpenSandbox source must be fixed at $commit; found $actualCommit."
    }
    foreach ($entry in $specs.GetEnumerator()) {
        Assert-Hash (Join-Path $SourceRoot $entry.Key) $entry.Value
        Assert-Hash (Join-Path "$root/vendor/opensandbox" $entry.Key) $entry.Value
    }
    Invoke-Native "Go toolchain check" { go version }
    if (-not $SkipRustTests) {
        Push-Location $root
        try {
            Invoke-Native "Rust OpenSandbox contract tests" { cargo test -p agentx-infrastructure opensandbox -- --nocapture }
        }
        finally {
            Pop-Location
        }
    }

    $moduleRoot = Join-Path $temporaryRoot "oracle"
    New-Item -ItemType Directory -Path $moduleRoot | Out-Null
    Copy-Item -LiteralPath "$root/scripts/opensandbox-oracle/main.go" -Destination (Join-Path $moduleRoot "main.go")
    Push-Location $moduleRoot
    try {
        Invoke-Native "Go oracle module init" { go mod init agentx/opensandbox-oracle }
        Invoke-Native "Go oracle SDK pin" { go mod edit "-require=github.com/alibaba/OpenSandbox/sdks/sandbox/go@v0.0.0" "-replace=github.com/alibaba/OpenSandbox/sdks/sandbox/go=$($SourceRoot.Replace('\', '/'))/sdks/sandbox/go" }
        Invoke-Native "Go oracle dependencies" { go mod tidy }
        $previousEndpoint = $env:AGENTX_OPENSANDBOX_ENDPOINT
        $previousApiKey = $env:AGENTX_OPENSANDBOX_API_KEY
        $previousImage = $env:AGENTX_OPENSANDBOX_IMAGE
        try {
            $env:AGENTX_OPENSANDBOX_ENDPOINT = $OpenSandboxEndpoint
            $env:AGENTX_OPENSANDBOX_API_KEY = $OpenSandboxApiKey
            $env:AGENTX_OPENSANDBOX_IMAGE = $OpenSandboxImage
            $oracleOutput = & go run .
            if ($LASTEXITCODE -ne 0) {
                throw "Official Go SDK oracle failed with exit code $LASTEXITCODE."
            }
        }
        finally {
            $env:AGENTX_OPENSANDBOX_ENDPOINT = $previousEndpoint
            $env:AGENTX_OPENSANDBOX_API_KEY = $previousApiKey
            $env:AGENTX_OPENSANDBOX_IMAGE = $previousImage
        }
    }
    finally {
        Pop-Location
    }
    $oracle = $oracleOutput | ConvertFrom-Json
    if (-not $oracle.uploadMatched -or -not $oracle.commandMatched -or -not $oracle.downloadMatched -or -not $oracle.listMatched -or -not $oracle.networkPolicyMatched -or -not $oracle.interruptMatched -or -not $oracle.terminated) {
        throw "Official Go SDK oracle returned an incomplete result."
    }
    $evidence = [ordered]@{
        commit = $commit
        lifecycleSpecSha256 = $specs["specs/sandbox-lifecycle.yml"]
        execdSpecSha256 = $specs["specs/execd-api.yaml"]
        source = if ($clonedSource) { "temporary-clone" } else { "local-fixed-clone" }
        oracle = $oracle
    } | ConvertTo-Json -Depth 8
    if ($ResultsPath) {
        $resultDirectory = Split-Path -Parent $ResultsPath
        if ($resultDirectory) {
            New-Item -ItemType Directory -Force -Path $resultDirectory | Out-Null
        }
        $evidence | Out-File -LiteralPath $ResultsPath -Encoding utf8
    }
    $evidence
}
finally {
    $resolvedTemp = [System.IO.Path]::GetFullPath($temporaryRoot)
    $systemTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolvedTemp.StartsWith($systemTemp, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedTemp).StartsWith("agentx-opensandbox-oracle-")) {
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
