param(
    [string]$Action,
    [string]$Target,
    [string]$BackupId,
    [string]$ConfigFile,
    [string]$RestoreTarget
)

@{
    status = "passed"
    recoveryPointUtc = [DateTimeOffset]::UtcNow.ToString("O")
    objectCount = 1
    contentSha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    schemaVersionObserved = "v2-07a-fixture"
} | ConvertTo-Json -Compress
