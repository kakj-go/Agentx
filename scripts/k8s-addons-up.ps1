param(
    [ValidateSet("all", "lightrag", "mem0")][string]$Addon = "all",
    [string]$Namespace = "agentx",
    [switch]$RebuildMem0
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$state = kubectl -n $Namespace get configmap agentx-deployment-state --ignore-not-found -o json | ConvertFrom-Json
$profile = if ($state) { $state.data.'profile.json' | ConvertFrom-Json -Depth 30 } else { Get-Content "$PSScriptRoot/../deploy/profiles/full-local.json" -Raw | ConvertFrom-Json -Depth 30 }
$profile.namespace = $Namespace
$full = Get-Content "$PSScriptRoot/../deploy/profiles/full-local.json" -Raw | ConvertFrom-Json -Depth 30
if ($Addon -in @("all", "lightrag")) { $profile.components.rag = $full.components.rag }
if ($Addon -in @("all", "mem0")) { $profile.components.memory = $full.components.memory }

$temporary = Join-Path ([IO.Path]::GetTempPath()) "agentx-addons-$PID.json"
try {
    [IO.File]::WriteAllText($temporary, ($profile | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
    $action = if ($state) { "Upgrade" } else { "Install" }
    & "$PSScriptRoot/deploy.ps1" -Action $action -ConfigFile $temporary -Target addons -NonInteractive
} finally { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
