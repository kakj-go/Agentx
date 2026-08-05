param([string]$Namespace = "agentx")

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
& "$PSScriptRoot/deploy.ps1" -Action Install -Profile Full -Namespace $Namespace -NonInteractive
