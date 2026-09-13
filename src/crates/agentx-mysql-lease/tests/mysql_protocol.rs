use std::{collections::BTreeSet, sync::Arc, time::Duration};

use agentx_mysql_lease::{FencingToken, LeaseError, LeaseOwner, require_single_lease_write};
use anyhow::Result;
use sqlx::{MySqlPool, Row, mysql::MySqlPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::Barrier;

#[derive(Clone, Copy)]
enum FixturePlane {
    Control,
    Runtime,
}

#[derive(Clone, Debug)]
struct Claimed {
    id: u64,
    owner: LeaseOwner,
    token: FencingToken,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 20)]
async fn control_and_runtime_fixtures_share_the_fenced_lease_protocol() {
    let container = GenericImage::new("mysql", "8.4")
        .with_exposed_port(3306.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "MySQL init process done. Ready for start up.",
        ))
        .with_wait_for(WaitFor::message_on_stderr("ready for connections"))
        .with_env_var("MYSQL_DATABASE", "agentx_lease")
        .with_env_var("MYSQL_USER", "agentx")
        .with_env_var("MYSQL_PASSWORD", "agentx-test-password")
        .with_env_var("MYSQL_ROOT_PASSWORD", "agentx-root-password")
        .start()
        .await
        .expect("MySQL container should start");
    let port = container
        .get_host_port_ipv4(3306.tcp())
        .await
        .expect("mapped MySQL port");
    let pool = connect_with_retry(port).await;
    create_fixture_tables(&pool).await.unwrap();

    for plane in [FixturePlane::Control, FixturePlane::Runtime] {
        seed(&pool, plane, 40).await.unwrap();
        concurrent_claims_are_unique(&pool, plane).await;
        fencing_and_expired_takeover(&pool, plane).await;
        pod_clock_skew_cannot_change_database_lease_decisions(&pool, plane).await;
        forced_claimant_termination_is_recovered_by_fencing(&pool, plane).await;
    }
}

async fn pod_clock_skew_cannot_change_database_lease_decisions(
    pool: &MySqlPool,
    plane: FixturePlane,
) {
    let claim = claim_one(pool, plane)
        .await
        .unwrap()
        .expect("clock-skew fixture claim");
    let fictitious_slow_pod_clock = time::OffsetDateTime::UNIX_EPOCH;
    let fictitious_fast_pod_clock =
        time::OffsetDateTime::UNIX_EPOCH + time::Duration::days(365 * 200);

    // Neither local value is passed into SQL. MySQL UTC_TIMESTAMP(6) is the
    // sole authority for heartbeat and completion eligibility.
    assert!(fictitious_slow_pod_clock < fictitious_fast_pod_clock);
    heartbeat(pool, plane, &claim).await.unwrap();
    complete(pool, plane, &claim).await.unwrap();
}

async fn forced_claimant_termination_is_recovered_by_fencing(
    pool: &MySqlPool,
    plane: FixturePlane,
) {
    let pool_for_claimant = pool.clone();
    let claimant = tokio::spawn(async move {
        let claim = claim_one(&pool_for_claimant, plane)
            .await
            .unwrap()
            .expect("forced-termination fixture claim");
        std::future::pending::<()>().await;
        claim
    });

    let id = wait_for_running_claim(pool, plane).await;
    claimant.abort();
    assert!(claimant.await.unwrap_err().is_cancelled());
    let abandoned = load_claim(pool, plane, id).await.unwrap();

    expire(pool, plane, id).await.unwrap();
    let replacement = claim_by_id(pool, plane, id)
        .await
        .unwrap()
        .expect("replacement takes over abandoned lease");
    assert_ne!(replacement.owner, abandoned.owner);
    assert_eq!(replacement.token, FencingToken(abandoned.token.0 + 1));
    assert_eq!(
        heartbeat(pool, plane, &abandoned).await,
        Err(LeaseError::LeaseLost)
    );
    assert_eq!(
        complete(pool, plane, &abandoned).await,
        Err(LeaseError::LeaseLost)
    );
    complete(pool, plane, &replacement).await.unwrap();
}

async fn wait_for_running_claim(pool: &MySqlPool, plane: FixturePlane) -> u64 {
    for _ in 0..100 {
        let id = match plane {
            FixturePlane::Control => sqlx::query_scalar(
                "SELECT id FROM control_claim_fixture WHERE status='running' ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .unwrap(),
            FixturePlane::Runtime => sqlx::query_scalar(
                "SELECT id FROM runtime_claim_fixture WHERE status='running' ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .unwrap(),
        };
        if let Some(id) = id {
            return id;
        }
        tokio::task::yield_now().await;
    }
    panic!("claimant did not persist its lease before forced termination")
}

async fn load_claim(pool: &MySqlPool, plane: FixturePlane, id: u64) -> Result<Claimed> {
    let row = match plane {
        FixturePlane::Control => {
            sqlx::query("SELECT locked_by,fencing_token FROM control_claim_fixture WHERE id=?")
                .bind(id)
                .fetch_one(pool)
                .await?
        }
        FixturePlane::Runtime => {
            sqlx::query("SELECT locked_by,fencing_token FROM runtime_claim_fixture WHERE id=?")
                .bind(id)
                .fetch_one(pool)
                .await?
        }
    };
    Ok(Claimed {
        id,
        owner: LeaseOwner(row.try_get("locked_by")?),
        token: FencingToken(row.try_get("fencing_token")?),
    })
}

async fn concurrent_claims_are_unique(pool: &MySqlPool, plane: FixturePlane) {
    let barrier = Arc::new(Barrier::new(20));
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let pool = pool.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            claim_one(&pool, plane).await.unwrap()
        }));
    }
    let mut ids = BTreeSet::new();
    for task in tasks {
        let claim = task.await.unwrap().expect("each contender claims a row");
        assert!(ids.insert(claim.id), "a row was claimed twice");
        assert_eq!(claim.token, FencingToken(1));
    }
    assert_eq!(ids.len(), 20);
}

async fn fencing_and_expired_takeover(pool: &MySqlPool, plane: FixturePlane) {
    let first = claim_one(pool, plane)
        .await
        .unwrap()
        .expect("initial claim");
    heartbeat(pool, plane, &first).await.unwrap();

    expire(pool, plane, first.id).await.unwrap();
    assert_eq!(
        complete(pool, plane, &first).await,
        Err(LeaseError::LeaseLost)
    );

    let second = claim_by_id(pool, plane, first.id)
        .await
        .unwrap()
        .expect("expired lease takeover");
    assert_ne!(second.owner, first.owner);
    assert_eq!(second.token, FencingToken(first.token.0 + 1));
    assert_eq!(fail(pool, plane, &first).await, Err(LeaseError::LeaseLost));
    complete(pool, plane, &second).await.unwrap();
    assert_eq!(
        complete(pool, plane, &second).await,
        Err(LeaseError::LeaseLost)
    );
}

async fn create_fixture_tables(pool: &MySqlPool) -> Result<()> {
    sqlx::query("CREATE TABLE control_claim_fixture (id BIGINT UNSIGNED PRIMARY KEY, status ENUM('ready','running','complete','failed') NOT NULL DEFAULT 'ready', available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6), locked_by BINARY(16) NULL, locked_until TIMESTAMP(6) NULL, fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0, INDEX idx_control_claim (status,available_at,locked_until))")
        .execute(pool).await?;
    sqlx::query("CREATE TABLE runtime_claim_fixture (id BIGINT UNSIGNED PRIMARY KEY, status ENUM('ready','running','complete','failed') NOT NULL DEFAULT 'ready', available_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6), locked_by BINARY(16) NULL, locked_until TIMESTAMP(6) NULL, fencing_token BIGINT UNSIGNED NOT NULL DEFAULT 0, INDEX idx_runtime_claim (status,available_at,locked_until))")
        .execute(pool).await?;
    Ok(())
}

async fn seed(pool: &MySqlPool, plane: FixturePlane, count: u64) -> Result<()> {
    for id in 1..=count {
        match plane {
            FixturePlane::Control => {
                sqlx::query("INSERT INTO control_claim_fixture(id) VALUES(?)")
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            FixturePlane::Runtime => {
                sqlx::query("INSERT INTO runtime_claim_fixture(id) VALUES(?)")
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(())
}

async fn claim_one(pool: &MySqlPool, plane: FixturePlane) -> Result<Option<Claimed>> {
    claim(pool, plane, None).await
}

async fn claim_by_id(pool: &MySqlPool, plane: FixturePlane, id: u64) -> Result<Option<Claimed>> {
    claim(pool, plane, Some(id)).await
}

async fn claim(pool: &MySqlPool, plane: FixturePlane, id: Option<u64>) -> Result<Option<Claimed>> {
    let owner = LeaseOwner::new();
    let mut transaction = pool.begin().await?;
    let row = match (plane, id) {
        (FixturePlane::Control, None) => sqlx::query("SELECT id,fencing_token FROM control_claim_fixture WHERE status IN ('ready','running') AND available_at<=UTC_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED").fetch_optional(&mut *transaction).await?,
        (FixturePlane::Runtime, None) => sqlx::query("SELECT id,fencing_token FROM runtime_claim_fixture WHERE status IN ('ready','running') AND available_at<=UTC_TIMESTAMP(6) AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED").fetch_optional(&mut *transaction).await?,
        (FixturePlane::Control, Some(id)) => sqlx::query("SELECT id,fencing_token FROM control_claim_fixture WHERE id=? AND status IN ('ready','running') AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) FOR UPDATE SKIP LOCKED").bind(id).fetch_optional(&mut *transaction).await?,
        (FixturePlane::Runtime, Some(id)) => sqlx::query("SELECT id,fencing_token FROM runtime_claim_fixture WHERE id=? AND status IN ('ready','running') AND (locked_until IS NULL OR locked_until<=UTC_TIMESTAMP(6)) FOR UPDATE SKIP LOCKED").bind(id).fetch_optional(&mut *transaction).await?,
    };
    let Some(row) = row else {
        transaction.commit().await?;
        return Ok(None);
    };
    let id: u64 = row.try_get("id")?;
    let token = FencingToken(row.try_get::<u64, _>("fencing_token")? + 1);
    let result = match plane {
        FixturePlane::Control => sqlx::query("UPDATE control_claim_fixture SET status='running',locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6), INTERVAL 30 SECOND),fencing_token=? WHERE id=?").bind(owner.0).bind(token.0).bind(id).execute(&mut *transaction).await?,
        FixturePlane::Runtime => sqlx::query("UPDATE runtime_claim_fixture SET status='running',locked_by=?,locked_until=DATE_ADD(UTC_TIMESTAMP(6), INTERVAL 30 SECOND),fencing_token=? WHERE id=?").bind(owner.0).bind(token.0).bind(id).execute(&mut *transaction).await?,
    };
    require_single_lease_write(result.rows_affected())?;
    transaction.commit().await?;
    Ok(Some(Claimed { id, owner, token }))
}

async fn heartbeat(
    pool: &MySqlPool,
    plane: FixturePlane,
    claim: &Claimed,
) -> Result<(), LeaseError> {
    let result = match plane {
        FixturePlane::Control => sqlx::query("UPDATE control_claim_fixture SET locked_until=DATE_ADD(UTC_TIMESTAMP(6), INTERVAL 30 SECOND) WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
        FixturePlane::Runtime => sqlx::query("UPDATE runtime_claim_fixture SET locked_until=DATE_ADD(UTC_TIMESTAMP(6), INTERVAL 30 SECOND) WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
    }.map_err(|_| LeaseError::LeaseLost)?;
    require_single_lease_write(result.rows_affected())
}

async fn complete(
    pool: &MySqlPool,
    plane: FixturePlane,
    claim: &Claimed,
) -> Result<(), LeaseError> {
    mutate_terminal(pool, plane, claim, "complete").await
}

async fn fail(pool: &MySqlPool, plane: FixturePlane, claim: &Claimed) -> Result<(), LeaseError> {
    mutate_terminal(pool, plane, claim, "failed").await
}

async fn mutate_terminal(
    pool: &MySqlPool,
    plane: FixturePlane,
    claim: &Claimed,
    state: &str,
) -> Result<(), LeaseError> {
    let result = match (plane, state) {
        (FixturePlane::Control, "complete") => sqlx::query("UPDATE control_claim_fixture SET status='complete',locked_by=NULL,locked_until=NULL WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
        (FixturePlane::Control, _) => sqlx::query("UPDATE control_claim_fixture SET status='failed',locked_by=NULL,locked_until=NULL WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
        (FixturePlane::Runtime, "complete") => sqlx::query("UPDATE runtime_claim_fixture SET status='complete',locked_by=NULL,locked_until=NULL WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
        (FixturePlane::Runtime, _) => sqlx::query("UPDATE runtime_claim_fixture SET status='failed',locked_by=NULL,locked_until=NULL WHERE id=? AND status='running' AND locked_by=? AND fencing_token=? AND locked_until>UTC_TIMESTAMP(6)").bind(claim.id).bind(claim.owner.0).bind(claim.token.0).execute(pool).await,
    }.map_err(|_| LeaseError::LeaseLost)?;
    require_single_lease_write(result.rows_affected())
}

async fn expire(pool: &MySqlPool, plane: FixturePlane, id: u64) -> Result<()> {
    match plane {
        FixturePlane::Control => sqlx::query("UPDATE control_claim_fixture SET locked_until=DATE_SUB(UTC_TIMESTAMP(6), INTERVAL 1 MICROSECOND) WHERE id=?").bind(id).execute(pool).await?,
        FixturePlane::Runtime => sqlx::query("UPDATE runtime_claim_fixture SET locked_until=DATE_SUB(UTC_TIMESTAMP(6), INTERVAL 1 MICROSECOND) WHERE id=?").bind(id).execute(pool).await?,
    };
    Ok(())
}

async fn connect_with_retry(port: u16) -> MySqlPool {
    let url = format!("mysql://agentx:agentx-test-password@127.0.0.1:{port}/agentx_lease");
    let mut last_error = None;
    for _ in 0..30 {
        match MySqlPoolOptions::new()
            .max_connections(24)
            .connect(&url)
            .await
        {
            Ok(pool) => return pool,
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("connect to fixture MySQL: {last_error:?}");
}
