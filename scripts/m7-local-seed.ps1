param(
    [Parameter(Mandatory = $true)][string]$Namespace,
    [Parameter(Mandatory = $true)][string]$BaseUrl,
    [Parameter(Mandatory = $true)][ValidateSet("m6", "m7")][string]$Phase
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$api = "$($BaseUrl.TrimEnd('/'))/api/v1"
$gateway = "$($BaseUrl.TrimEnd('/'))/gateway/v1"
$suffix = [DateTimeOffset]::UtcNow.ToString("yyyyMMddHHmmssfff")

function Invoke-Agentx(
    [string]$Path,
    [string]$Method = "GET",
    $Body = $null,
    [hashtable]$Headers = @{}
) {
    $parameters = @{
        Uri = "$api$Path"
        Method = $Method
        Headers = $Headers
        TimeoutSec = 90
    }
    if ($null -ne $Body) {
        $parameters.ContentType = "application/json"
        $parameters.Body = $Body | ConvertTo-Json -Depth 50 -Compress
    }
    Invoke-RestMethod @parameters
}

function Wait-Execution([string]$ExecutionId, [hashtable]$Headers) {
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(180)
    do {
        Start-Sleep -Milliseconds 500
        $execution = Invoke-Agentx "/executions/$ExecutionId" "GET" $null $Headers
    } while ($execution.status -notin @("succeeded", "failed", "cancelled") -and [DateTimeOffset]::UtcNow -lt $deadline)
    if ($execution.status -ne "succeeded") { throw "Seed Execution $ExecutionId ended as $($execution.status)." }
    $execution
}

$bootstrap = Invoke-Agentx "/bootstrap" "POST" @{
    companyName = "Agentx M7 Local $Phase"
    adminUsername = "admin"
    adminDisplayName = "M7 Local Admin"
    password = "agentx-m7-local-admin-password"
    locale = "zh-CN"
    timezone = "Asia/Shanghai"
}
$accessToken = [string]$bootstrap.accessToken
if (-not $accessToken) { throw "Bootstrap did not return an access token." }
$headers = @{ Authorization = "Bearer $accessToken" }

$environment = @(Invoke-Agentx "/environments" "GET" $null $headers | Where-Object code -eq "development") | Select-Object -First 1
if (-not $environment) { throw "The built-in development Environment does not exist." }

$workflow = Invoke-Agentx "/workflows" "POST" @{
    name = "M7 Local $Phase $suffix"
    description = "INT-009 local rolling upgrade baseline"
    visibility = "company"
} $headers
$draft = Invoke-Agentx "/workflows/$($workflow.id)/draft" "GET" $null $headers
$saved = Invoke-Agentx "/workflows/$($workflow.id)/draft" "PUT" @{
    expectedRevision = $draft.revision
    definition = @{
        schemaVersion = "3.0"
        nodes = @(
            @{
                id = "manual-trigger"
                type = "manual_trigger"
                typeVersion = 1
                name = "Manual Trigger"
                disabled = $false
                parameters = @{}
                resourceReferences = @()
                settings = @{}
            },
            @{
                id = "set-result"
                type = "set"
                typeVersion = 1
                name = "Set Result"
                disabled = $false
                parameters = @{ values = @{ accepted = $true; phase = $Phase }; keepOnlySet = $false }
                resourceReferences = @()
                settings = @{}
            }
        )
        connections = @(
            @{
                id = "manual-to-set"
                sourceNodeId = "manual-trigger"
                sourceHandle = "main"
                targetNodeId = "set-result"
                targetHandle = "main"
                order = 0
            }
        )
        settings = @{ executionOrder = "deterministic" }
    }
    editorDocument = @{
        nodeLayouts = @(
            @{ nodeId = "manual-trigger"; x = 80; y = 160 },
            @{ nodeId = "set-result"; x = 360; y = 160 }
        )
        bindingLayouts = @()
        edges = @(@{ edgeId = "manual-to-set" })
        bindingEdges = @()
        annotations = @()
        groups = @()
        viewport = @{ x = 0; y = 0; zoom = 1 }
    }
} $headers
$version = Invoke-Agentx "/workflows/$($workflow.id)/versions" "POST" @{ draftRevision = $saved.revision } $headers
Invoke-Agentx "/workflows/$($workflow.id)/deployments" "POST" @{
    workflowVersionId = $version.id
    environmentId = $environment.id
} $headers | Out-Null

$application = Invoke-Agentx "/applications" "POST" @{
    workflowId = $workflow.id
    name = "M7 Local $Phase $suffix"
    slug = "m7-local-$Phase-$suffix"
    description = "INT-009 local rolling upgrade probe"
    visibility = "company"
} $headers
Invoke-Agentx "/applications/$($application.id)/deployments" "POST" @{
    workflowVersionId = $version.id
    environmentId = $environment.id
    inputSchema = @{}
    outputSchema = @{}
    outputExpression = $null
    sessionVersionPolicy = "pinned"
} $headers | Out-Null
$apiKey = Invoke-Agentx "/applications/$($application.id)/api-keys" "POST" @{ name = "M7 local acceptance" } $headers
if (-not $apiKey.secret) { throw "Application API Key creation did not return the one-time secret." }

$executionRequest = Invoke-Agentx "/workflow-versions/$($version.id)/executions" "POST" @{
    input = @{ source = "m7-local-seed"; phase = $Phase }
    idempotencyKey = "m7-local-seed-$Phase-$suffix"
} $headers
$execution = Wait-Execution ([string]$executionRequest.executionId) $headers
$invocationId = $null

if ($Phase -eq "m7") {
    $invocationHeaders = @{
        Authorization = "Bearer $($apiKey.secret)"
        "Idempotency-Key" = "m7-local-invocation-$suffix"
    }
    $invocation = Invoke-RestMethod -Uri "$gateway/applications/$($application.slug)/invocations" -Method Post -Headers $invocationHeaders -ContentType "application/json" -Body (@{ input = @{ source = "m7-local-seed" } } | ConvertTo-Json -Compress) -TimeoutSec 90
    $invocationId = [string]$invocation.id
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(180)
    do {
        Start-Sleep -Milliseconds 500
        $terminal = Invoke-RestMethod -Uri "$gateway/invocations/$invocationId" -Headers @{ Authorization = "Bearer $($apiKey.secret)" } -TimeoutSec 90
    } while ($terminal.status -notin @("completed", "failed", "cancelled") -and [DateTimeOffset]::UtcNow -lt $deadline)
    if ($terminal.status -ne "completed" -or -not $terminal.executionId) {
        throw "Seed Application Invocation ended as $($terminal.status)."
    }
}

[ordered]@{
    phase = $Phase
    namespace = $Namespace
    workflowId = [string]$workflow.id
    workflowVersionId = [string]$version.id
    applicationId = [string]$application.id
    applicationSlug = [string]$application.slug
    executionId = [string]$execution.id
    invocationId = $invocationId
    bearerToken = [string]$apiKey.secret
} | ConvertTo-Json -Depth 10 -Compress
