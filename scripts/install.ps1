[CmdletBinding()]
param(
    [ValidateSet("Control", "Runtime", "Observability", "Dependencies", "All")]
    [string]$Target = "All",
    [string]$ConfigFile = "deploy/profiles/v2-dockerhub-beta.json",
    [string]$RunId = "",
    [switch]$CleanupOnFailure
)

$ErrorActionPreference = "Stop"
$deployScript = Join-Path $PSScriptRoot "deploy-v2.ps1"

function Invoke-AgentxDeployPhase {
    param(
        [ValidateSet("Validate", "Install", "Doctor")]
        [string]$Action,
        [int]$Step
    )

    $labels = @{
        Validate = "校验部署配置"
        Install = "安装 Agentx"
        Doctor = "执行安装后健康检查"
    }
    Write-Host "[$Step/3] $($labels[$Action])"

    $arguments = @{
        Action = $Action
        Target = $Target
        ConfigFile = $ConfigFile
    }
    if ($RunId) {
        $arguments.RunId = $RunId
    }
    if ($Action -eq "Install" -and $CleanupOnFailure) {
        $arguments.CleanupOnFailure = $true
    }

    & $deployScript @arguments
    if (-not $?) {
        throw "Agentx $Action 阶段执行失败。"
    }
}

Invoke-AgentxDeployPhase -Action Validate -Step 1
Invoke-AgentxDeployPhase -Action Install -Step 2
Invoke-AgentxDeployPhase -Action Doctor -Step 3

Write-Host "Agentx 安装完成，部署配置和运行依赖均已通过检查。"
