param(
    [ValidateSet("all", "lightrag", "mem0")][string]$Addon = "all",
    [string]$Namespace = "agentx",
    [switch]$DeleteData
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$state = kubectl -n $Namespace get configmap agentx-deployment-state -o json | ConvertFrom-Json
$profile = $state.data.'profile.json' | ConvertFrom-Json -Depth 30
if ($Addon -in @("all", "lightrag")) { $profile.components.rag.mode = "disabled" }
if ($Addon -in @("all", "mem0")) { $profile.components.memory.mode = "disabled" }
$temporary = Join-Path ([IO.Path]::GetTempPath()) "agentx-addons-$PID.json"
try {
    [IO.File]::WriteAllText($temporary, ($profile | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
    & "$PSScriptRoot/deploy.ps1" -Action Upgrade -ConfigFile $temporary -Target addons -NonInteractive
    if ($DeleteData) {
        $claims = @()
        if ($Addon -in @("all", "lightrag")) { $claims += "lightrag-data" }
        if ($Addon -in @("all", "mem0")) { $claims += @("mem0-history-data", "mem0-postgres-data") }
        foreach ($claim in $claims) {
            $item = kubectl -n $Namespace get pvc $claim --ignore-not-found -o json | ConvertFrom-Json
            if ($item -and $item.metadata.labels.'app.kubernetes.io/managed-by' -eq "agentx-deploy") { kubectl -n $Namespace delete pvc $claim --ignore-not-found }
        }
    }
} finally { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
