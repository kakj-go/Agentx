use anyhow::{Context, Result, bail, ensure};

pub(crate) fn parse_migration_upper_bound(arguments: &[String]) -> Result<Option<i64>> {
    match arguments {
        [] => Ok(None),
        [flag, value] if flag == "--through" => {
            let version = value
                .parse::<i64>()
                .with_context(|| format!("invalid migration version '{value}'"))?;
            ensure!(version > 0, "migration version must be positive");
            Ok(Some(version))
        }
        _ => bail!("usage: platform-api migrate [--through <version>]"),
    }
}

pub(crate) async fn validate_m7_contract_inputs(pool: &sqlx::MySqlPool) -> Result<()> {
    let migration_table_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if migration_table_exists == 0 {
        return Ok(());
    }
    let expand_applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=16 AND success=TRUE",
    )
    .fetch_one(pool)
    .await?;
    let contract_applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=17 AND success=TRUE",
    )
    .fetch_one(pool)
    .await?;
    if expand_applied == 0 || contract_applied > 0 {
        return Ok(());
    }

    let invalid_credentials: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM credential_secret_versions WHERE NOT ((provider='local_encrypted' AND algorithm IS NOT NULL AND key_id IS NOT NULL AND nonce IS NOT NULL AND ciphertext IS NOT NULL) OR (provider<>'local_encrypted' AND secret_ref IS NOT NULL AND provider_version IS NOT NULL))",
    )
    .fetch_one(pool)
    .await?;
    ensure!(
        invalid_credentials == 0,
        "M7 contract blocked by {invalid_credentials} invalid credential secret rows"
    );
    let invalid_webhooks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM application_webhooks WHERE NOT ((secret_provider='local_encrypted' AND secret_algorithm IS NOT NULL AND secret_key_id IS NOT NULL AND secret_nonce IS NOT NULL AND secret_ciphertext IS NOT NULL) OR (secret_provider<>'local_encrypted' AND secret_ref IS NOT NULL AND secret_provider_version IS NOT NULL))",
    )
    .fetch_one(pool)
    .await?;
    ensure!(
        invalid_webhooks == 0,
        "M7 contract blocked by {invalid_webhooks} invalid webhook secret rows"
    );
    let contract_table_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema=DATABASE() AND table_name='release_schema_contract'",
    )
    .fetch_one(pool)
    .await?;
    if contract_table_exists > 0 {
        let conflicts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM release_schema_contract WHERE contract_name='m7-runtime-integration' AND (schema_version<>'17' OR minimum_application_version<>'0.1.0')",
        )
        .fetch_one(pool)
        .await?;
        ensure!(
            conflicts == 0,
            "M7 release schema contract contains a conflicting marker"
        );
    }
    Ok(())
}
