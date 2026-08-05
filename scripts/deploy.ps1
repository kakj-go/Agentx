param(
    [Parameter(Mandatory)]
    [ValidateSet("Doctor", "Install", "Upgrade", "Status", "Uninstall")]
    [string]$Action,
    [ValidateSet("Full", "Custom")]
    [string]$Profile = "Full",
    [string]$ConfigFile,
    [string]$Namespace,
    [switch]$NonInteractive,
    [switch]$DryRun,
    [ValidateSet("all", "services", "infrastructure", "addons", "sandbox", "ingress")]
    [string]$Target = "all",
    [switch]$DeleteData,
    [switch]$DeleteNamespace,
    [switch]$RotateSecrets
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Import-Module "$PSScriptRoot/deploy/Agentx.Deployment.psm1" -Force
Invoke-AgentxDeployment @PSBoundParameters
