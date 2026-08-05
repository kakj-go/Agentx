param(
    [string]$Namespace = "agentx",
    [switch]$DeleteData,
    [switch]$DeleteNamespace
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
& "$PSScriptRoot/deploy.ps1" -Action Uninstall -Namespace $Namespace -Target all -DeleteData:$DeleteData -DeleteNamespace:$DeleteNamespace -NonInteractive
