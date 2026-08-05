param(
    [string]$Namespace = "agentx",
    [string]$ResultsPath = "",
    [switch]$PreserveFixtureData,
    [switch]$CleanupOnly
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$runId = [Guid]::NewGuid().ToString("N")
$ragPort = 19621
$memoryPort = 18000
$ragForward = $null
$memoryForward = $null
$ragDocumentId = $null
$memoryId = $null
$fileSource = if ($PreserveFixtureData) { "m5-worker-fixture.txt" } else { "m5-addon-$runId.txt" }
$memoryUser = if ($PreserveFixtureData) { "m5-worker-fixture" } else { "m5-addon-$runId" }
$ragApiKeyEncoded = kubectl -n $Namespace get secret agentx-lightrag-secrets -o "jsonpath={.data.LIGHTRAG_API_KEY}"
if (-not $ragApiKeyEncoded) {
    throw "agentx-lightrag-secrets is missing LIGHTRAG_API_KEY."
}
$ragHeaders = @{ "X-API-Key" = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($ragApiKeyEncoded)) }
$completed = $false

function Wait-TcpPort([int]$Port) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $client.Connect("127.0.0.1", $Port)
            return
        }
        catch {
            Start-Sleep -Seconds 1
        }
        finally {
            $client.Dispose()
        }
    }
    throw "Timed out waiting for local port $Port."
}

function Start-Forward([string]$Service, [int]$LocalPort, [int]$RemotePort) {
    $logRoot = if ($ResultsPath) { Split-Path -Parent $ResultsPath } else { [System.IO.Path]::GetTempPath() }
    New-Item -ItemType Directory -Force -Path $logRoot | Out-Null
    $stdout = Join-Path $logRoot "$Service-port-forward.out.log"
    $stderr = Join-Path $logRoot "$Service-port-forward.err.log"
    $process = Start-Process kubectl -ArgumentList @("-n", $Namespace, "port-forward", "service/$Service", "${LocalPort}:${RemotePort}") -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Wait-TcpPort $LocalPort
    return $process
}

function Wait-LightRagDocument {
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        $documents = Invoke-RestMethod -Uri "http://127.0.0.1:$ragPort/documents?status=processed" -Headers $ragHeaders
        $match = @($documents.statuses.processed | Where-Object { $_.file_path -eq $fileSource }) | Select-Object -First 1
        if ($match) {
            return $match
        }
        Start-Sleep -Seconds 1
    }
    throw "LightRAG did not process $fileSource before timeout."
}

function Wait-LightRagDeletion {
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        $documents = Invoke-RestMethod -Uri "http://127.0.0.1:$ragPort/documents?status=processed" -Headers $ragHeaders
        if (-not @($documents.statuses.processed | Where-Object { $_.id -eq $ragDocumentId })) {
            return
        }
        Start-Sleep -Seconds 1
    }
    throw "LightRAG did not delete $ragDocumentId before timeout."
}

function Stop-Forwards {
    foreach ($process in @($ragForward, $memoryForward)) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
        }
    }
}

function Remove-FixtureData {
    if ($ragDocumentId) {
        $deleteBody = @{ doc_ids = @($ragDocumentId); delete_file = $false; delete_llm_cache = $false } | ConvertTo-Json -Compress
        Invoke-RestMethod -Method Delete -Uri "http://127.0.0.1:$ragPort/documents/delete_document" -Headers $ragHeaders -ContentType "application/json" -Body $deleteBody | Out-Null
        Wait-LightRagDeletion
        $script:ragDocumentId = $null
    }
    if ($memoryId) {
        Invoke-RestMethod -Method Delete -Uri "http://127.0.0.1:$memoryPort/memories/$memoryId" | Out-Null
        $script:memoryId = $null
    }
}

function Assert-DeniedWriteAbsent {
    $documents = Invoke-RestMethod -Uri "http://127.0.0.1:$ragPort/documents" -Headers $ragHeaders
    $allDocuments = @($documents.statuses.PSObject.Properties | ForEach-Object { @($_.Value) })
    if (@($allDocuments | Where-Object { $_.file_path -eq "m5-denied-write.txt" }).Count -ne 0) {
        throw "The read-scoped RAG write reached LightRAG."
    }
}

kubectl -n $Namespace rollout status deployment/lightrag --timeout=300s
kubectl -n $Namespace rollout status deployment/mem0-postgres --timeout=300s
kubectl -n $Namespace rollout status deployment/mem0 --timeout=300s

if ($CleanupOnly) {
    if (-not $ResultsPath -or -not (Test-Path -LiteralPath $ResultsPath)) {
        throw "CleanupOnly requires an existing ResultsPath."
    }
    $evidence = Get-Content -Raw -LiteralPath $ResultsPath | ConvertFrom-Json
    $ragDocumentId = [string]$evidence.lightragDocumentId
    $fileSource = [string]$evidence.lightragFileSource
    $memoryId = [string]$evidence.mem0MemoryId
    try {
        $ragForward = Start-Forward "lightrag" $ragPort 9621
        $memoryForward = Start-Forward "mem0" $memoryPort 8000
        Assert-DeniedWriteAbsent
        Remove-FixtureData
    }
    finally {
        Stop-Forwards
    }
    return
}

try {
    $ragForward = Start-Forward "lightrag" $ragPort 9621
    $memoryForward = Start-Forward "mem0" $memoryPort 8000

    $health = Invoke-RestMethod -Uri "http://127.0.0.1:$ragPort/health" -Headers $ragHeaders
    if ($health.core_version -ne "1.5.5" -or $health.configuration.embedding_model -ne "echo-embedding-8") {
        throw "LightRAG fixed version or embedding configuration drifted."
    }
    $unauthorized = 0
    try {
        Invoke-WebRequest -Method Post -Uri "http://127.0.0.1:$ragPort/query" -ContentType "application/json" -Body '{"query":"Which adapter does Agentx use?","mode":"naive"}' -ErrorAction Stop | Out-Null
    }
    catch {
        $unauthorized = [int]$_.Exception.Response.StatusCode
    }
    if ($unauthorized -ne 403) {
        throw "LightRAG accepted a query without X-API-Key."
    }
    $insertBody = @{ text = "Agentx M5 uses a direct Rust OpenSandbox adapter."; file_source = $fileSource } | ConvertTo-Json -Compress
    $insert = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$ragPort/documents/text" -Headers $ragHeaders -ContentType "application/json" -Body $insertBody
    if ($insert.status -ne "success") {
        throw "LightRAG insertion was not accepted."
    }
    $document = Wait-LightRagDocument
    $ragDocumentId = [string]$document.id
    $queryBody = @{ query = "Which adapter does Agentx M5 use?"; mode = "naive"; include_references = $true } | ConvertTo-Json -Compress
    $query = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$ragPort/query" -Headers $ragHeaders -ContentType "application/json" -Body $queryBody
    if (-not @($query.references | Where-Object { $_.file_path -eq $fileSource })) {
        throw "LightRAG query did not return the inserted document reference."
    }

    $openApi = Invoke-RestMethod -Uri "http://127.0.0.1:$memoryPort/openapi.json"
    if ($openApi.info.version -ne "1.0.0" -or -not $openApi.paths.'/memories' -or -not $openApi.paths.'/search') {
        throw "Mem0 v2.0.15 REST contract drifted."
    }
    $memoryConfig = Invoke-RestMethod -Uri "http://127.0.0.1:$memoryPort/configure"
    if ($memoryConfig.vector_store.provider -ne "pgvector" -or $memoryConfig.vector_store.config.host -ne "mem0-postgres") {
        throw "Mem0 bundled pgvector configuration did not target mem0-postgres."
    }
    $addBody = @{ messages = @(@{ role = "user"; content = "Agentx M5 uses a Rust OpenSandbox adapter." }); user_id = $memoryUser; infer = $false } | ConvertTo-Json -Compress -Depth 10
    $added = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$memoryPort/memories" -ContentType "application/json" -Body $addBody
    $memoryId = [string]$added.results[0].id
    if (-not $memoryId) {
        throw "Mem0 did not return a memory ID."
    }
    $searchBody = @{ query = "Which adapter does Agentx use?"; filters = @{ user_id = $memoryUser }; top_k = 5 } | ConvertTo-Json -Compress -Depth 10
    $search = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$memoryPort/search" -ContentType "application/json" -Body $searchBody
    if (-not @($search.results | Where-Object { $_.id -eq $memoryId })) {
        throw "Mem0 search did not return the inserted memory."
    }
    Invoke-RestMethod -Method Put -Uri "http://127.0.0.1:$memoryPort/memories/$memoryId" -ContentType "application/json" -Body '{"text":"Agentx M5 uses a direct Rust OpenSandbox adapter."}' | Out-Null
    $updated = Invoke-RestMethod -Uri "http://127.0.0.1:$memoryPort/memories/$memoryId"
    if ($updated.memory -ne "Agentx M5 uses a direct Rust OpenSandbox adapter.") {
        throw "Mem0 update was not visible through get."
    }

    $evidence = [ordered]@{
        runId = $runId
        lightragVersion = [string]$health.core_version
        lightragEmbeddingModel = [string]$health.configuration.embedding_model
        lightragUnauthorizedStatus = $unauthorized
        lightragDocumentId = $ragDocumentId
        lightragFileSource = $fileSource
        lightragReferenceMatched = $true
        mem0ApiVersion = [string]$openApi.info.version
        mem0VectorHost = [string]$memoryConfig.vector_store.config.host
        mem0MemoryId = $memoryId
        mem0User = $memoryUser
        mem0SearchMatched = $true
        mem0UpdateMatched = $true
    }
    $json = $evidence | ConvertTo-Json -Depth 10
    if ($ResultsPath) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ResultsPath) | Out-Null
        $json | Out-File -LiteralPath $ResultsPath -Encoding utf8
    }
    $json
    $completed = $true
}
finally {
    if (-not ($PreserveFixtureData -and $completed) -and ($ragDocumentId -or $memoryId)) {
        try {
            Remove-FixtureData
        }
        catch {
            Write-Warning "M5 Addon fixture cleanup failed: $_"
        }
    }
    Stop-Forwards
}
