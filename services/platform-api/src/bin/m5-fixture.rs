use agentx_domain::{WorkflowDefinition, canonical_content_hash};
use agentx_infrastructure::{
    config::InfrastructureSettings,
    credential::{CredentialKeyring, PlainSecret},
    mysql,
};
use agentx_runtime::{CompileContext, NodeRegistry, WorkflowCompiler};
use anyhow::{Context, Result};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{MySql, MySqlPool, Row, Transaction};
use uuid::Uuid;

struct Resources {
    credential: Uuid,
    rag_credential: Uuid,
    model: Uuid,
    model_version: Uuid,
    mcp_server: Uuid,
    mcp_server_version: Uuid,
    mcp_tool: Uuid,
    mcp_tool_version: Uuid,
    rag: Uuid,
    memory: Uuid,
    sandbox_profile: Uuid,
    sandbox_profile_version: Uuid,
    sandbox_image: String,
}

struct Snapshot {
    node_id: &'static str,
    resource_type: &'static str,
    resource_id: Uuid,
    resource_version_id: Option<Uuid>,
    operation: &'static str,
    value: Value,
}

#[derive(Clone, Copy)]
struct SandboxLimits {
    cpu_millis: u64,
    memory_bytes: u64,
    pids_limit: u64,
    disk_bytes: u64,
    timeout_seconds: u64,
    output_limit_bytes: u64,
}

impl SandboxLimits {
    const STANDARD: Self = Self {
        cpu_millis: 500,
        memory_bytes: 536_870_912,
        pids_limit: 128,
        disk_bytes: 1_073_741_824,
        timeout_seconds: 300,
        output_limit_bytes: 1_048_576,
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings = InfrastructureSettings::from_env()?;
    let pool = mysql::connect(&settings.mysql).await?;
    let owner = sqlx::query("SELECT u.tenant_id,u.id user_id,ud.department_id FROM users u JOIN user_departments ud ON ud.tenant_id=u.tenant_id AND ud.user_id=u.id WHERE u.username='admin' AND u.status='active' ORDER BY u.created_at LIMIT 1")
        .fetch_optional(&pool).await?.context("E2E company admin is missing")?;
    let tenant: Uuid = owner.try_get("tenant_id")?;
    let user: Uuid = owner.try_get("user_id")?;
    let department: Uuid = owner.try_get("department_id")?;
    let resources = ensure_resources(&pool, tenant, user, department).await?;
    let (javascript_profile, javascript_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &resources.sandbox_image,
        "javascript",
        "M5 JavaScript Fixture",
        SandboxLimits::STANDARD,
    )
    .await?;
    let (shell_profile, shell_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &resources.sandbox_image,
        "shell",
        "M5 Shell Fixture",
        SandboxLimits::STANDARD,
    )
    .await?;
    let browser_image = std::env::var("AGENTX_M5_BROWSER_IMAGE")
        .context("AGENTX_M5_BROWSER_IMAGE must be pinned by digest")?;
    anyhow::ensure!(
        browser_image.contains("@sha256:")
            && browser_image
                .rsplit('@')
                .next()
                .is_some_and(|digest| digest.len() == 71 && digest.starts_with("sha256:")),
        "AGENTX_M5_BROWSER_IMAGE must be pinned by sha256 digest"
    );
    let (browser_profile, browser_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &browser_image,
        "browser",
        "M5 Browser Fixture",
        SandboxLimits::STANDARD,
    )
    .await?;
    let partial_limits = SandboxLimits {
        output_limit_bytes: 32,
        ..SandboxLimits::STANDARD
    };
    let (partial_profile, partial_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &resources.sandbox_image,
        "python",
        "M5 Partial Output Fixture",
        partial_limits,
    )
    .await?;
    let memory_limits = SandboxLimits {
        memory_bytes: 67_108_864,
        ..SandboxLimits::STANDARD
    };
    let (memory_profile, memory_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &resources.sandbox_image,
        "python",
        "M5 Memory Limit Fixture",
        memory_limits,
    )
    .await?;
    let ttl_limits = SandboxLimits {
        timeout_seconds: 60,
        ..SandboxLimits::STANDARD
    };
    let (ttl_profile, ttl_profile_version) = ensure_sandbox_profile(
        &pool,
        tenant,
        user,
        department,
        &resources.sandbox_image,
        "python",
        "M5 Natural TTL Fixture",
        ttl_limits,
    )
    .await?;

    create_agent_workflow(&pool, tenant, user, department, &resources, false).await?;
    create_agent_workflow(&pool, tenant, user, department, &resources, true).await?;
    create_knowledge_workflows(&pool, tenant, user, department, &resources).await?;
    create_code_workflow(&pool, tenant, user, department, &resources).await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 JavaScript Code Fixture",
            node_name: "M5 JavaScript Code",
            runner: "javascript",
            source: "const fs = require('fs'); console.log('m5-javascript-ok'); fs.writeFileSync('/workspace/result.txt', 'm5-javascript-artifact-ok')",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: Some("/workspace/result.txt"),
            profile: javascript_profile,
            profile_version: javascript_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Browser Code Fixture",
            node_name: "M5 Browser Code",
            runner: "browser",
            source: "import asyncio\nfrom pathlib import Path\nfrom playwright.async_api import async_playwright\nPath('/home/playwright/page.html').write_text('<!doctype html><title>M5 Browser</title><main>m5-browser-page</main>', encoding='utf-8')\nasync def main():\n    async with async_playwright() as playwright:\n        browser = await playwright.chromium.launch(headless=True)\n        page = await browser.new_page()\n        await page.goto('file:///home/playwright/page.html')\n        print('m5-browser-title:' + await page.title())\n        print('m5-browser-text:' + (await page.locator('main').text_content() or ''))\n        await page.screenshot(path='/home/playwright/browser.png')\n        await browser.close()\nasyncio.run(main())",
            image: browser_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: Some("/home/playwright/browser.png"),
            profile: browser_profile,
            profile_version: browser_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Shell Code Fixture",
            node_name: "M5 Shell Code",
            runner: "shell",
            source: "printf 'm5-shell-ok\\n'; printf 'm5-shell-artifact-ok' > /workspace/result.txt",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: Some("/workspace/result.txt"),
            profile: shell_profile,
            profile_version: shell_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Credential Code Fixture",
            node_name: "M5 Credential Code",
            runner: "python",
            source: "import os\nfrom pathlib import Path\nsecret = Path(os.environ['M5_SECRET_FILE']).read_text(encoding='utf-8')\nassert secret == 'm5-model-secret'\nprint('m5-credential-ok')",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: None,
            profile: resources.sandbox_profile,
            profile_version: resources.sandbox_profile_version,
            network_policy: None,
            credential: true,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Partial Output Fixture",
            node_name: "M5 Partial Output Code",
            runner: "python",
            source: "import sys\nprint('m5-partial-stdout-' + 'o' * 256, flush=True)\nprint('m5-partial-stderr-' + 'e' * 256, file=sys.stderr, flush=True)",
            image: resources.sandbox_image.clone(),
            limits: partial_limits,
            expected_output_path: None,
            profile: partial_profile,
            profile_version: partial_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Memory Limit Fixture",
            node_name: "M5 Memory Limit Code",
            runner: "python",
            source: "print('m5-memory-limit-started', flush=True)\nvalue = bytearray(256 * 1024 * 1024)\nvalue[0] = 1\nprint(len(value))",
            image: resources.sandbox_image.clone(),
            limits: memory_limits,
            expected_output_path: None,
            profile: memory_profile,
            profile_version: memory_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        &pool,
        tenant,
        user,
        department,
        &resources,
        CodeFixture {
            workflow_name: "M5 Natural TTL Fixture",
            node_name: "M5 Natural TTL Code",
            runner: "python",
            source: "import time\nprint('m5-ttl-started', flush=True)\ntime.sleep(180)",
            image: resources.sandbox_image.clone(),
            limits: ttl_limits,
            expected_output_path: None,
            profile: ttl_profile,
            profile_version: ttl_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_network_workflows(&pool, tenant, user, department, &resources).await?;
    for (workflow_name, node_name) in [
        ("M5 Cancellation Fixture", "M5 Cancellation Code"),
        ("M5 Manager Restart Fixture", "M5 Manager Restart Code"),
    ] {
        create_code_fixture(
            &pool,
            tenant,
            user,
            department,
            &resources,
            CodeFixture {
                workflow_name,
                node_name,
                runner: "python",
                source: "import time\nprint('m5-long-command-started', flush=True)\ntime.sleep(120)",
                image: resources.sandbox_image.clone(),
                limits: SandboxLimits::STANDARD,
                expected_output_path: None,
                profile: resources.sandbox_profile,
                profile_version: resources.sandbox_profile_version,
                network_policy: None,
                credential: false,
            },
        )
        .await?;
    }
    Ok(())
}

async fn ensure_resources(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
) -> Result<Resources> {
    let credential = ensure_credential(pool, tenant, user, department).await?;
    let rag_credential = ensure_lightrag_credential(pool, tenant, user, department).await?;
    let (model, model_version) = ensure_model(pool, tenant, user, department, credential).await?;
    let (mcp_server, mcp_server_version, mcp_tool, mcp_tool_version) =
        ensure_mcp(pool, tenant, user, department).await?;
    let rag = ensure_rag(pool, tenant, department, rag_credential).await?;
    let memory = ensure_memory(pool, tenant, department).await?;
    let sandbox_image = std::env::var("AGENTX_M5_SANDBOX_IMAGE")
        .context("AGENTX_M5_SANDBOX_IMAGE must be pinned by digest")?;
    anyhow::ensure!(
        sandbox_image.contains("@sha256:")
            && sandbox_image
                .rsplit('@')
                .next()
                .is_some_and(|value| value.len() == 71),
        "AGENTX_M5_SANDBOX_IMAGE must be pinned by sha256 digest"
    );
    let (sandbox_profile, sandbox_profile_version) = ensure_sandbox_profile(
        pool,
        tenant,
        user,
        department,
        &sandbox_image,
        "python",
        "M5 Python Fixture",
        SandboxLimits::STANDARD,
    )
    .await?;
    Ok(Resources {
        credential,
        rag_credential,
        model,
        model_version,
        mcp_server,
        mcp_server_version,
        mcp_tool,
        mcp_tool_version,
        rag,
        memory,
        sandbox_profile,
        sandbox_profile_version,
        sandbox_image,
    })
}

async fn ensure_credential(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM credentials WHERE tenant_id=? AND name='M5 Model Fixture Credential'",
    )
    .bind(tenant)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    let keyring = CredentialKeyring::from_json(
        std::env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")?,
        &SecretString::from(std::env::var("AGENTX_CREDENTIAL_KEYS_JSON")?),
    )?;
    let encrypted = keyring.encrypt(
        &PlainSecret::new(b"m5-model-secret".to_vec()),
        format!("{tenant}/{id}/1").as_bytes(),
    )?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,'M5 Model Fixture Credential','bearer','****',?,?)")
        .bind(id).bind(tenant).bind(department).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,1,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant).bind(id).bind(encrypted.algorithm).bind(encrypted.key_id).bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

async fn ensure_lightrag_credential(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM credentials WHERE tenant_id=? AND name='M5 LightRAG Fixture Credential'",
    )
    .bind(tenant)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    let keyring = CredentialKeyring::from_json(
        std::env::var("AGENTX_CREDENTIAL_ACTIVE_KEY_ID")?,
        &SecretString::from(std::env::var("AGENTX_CREDENTIAL_KEYS_JSON")?),
    )?;
    let api_key = std::env::var("LIGHTRAG_API_KEY")
        .context("LIGHTRAG_API_KEY is required by the M5 LightRAG fixture")?;
    let encrypted = keyring.encrypt(
        &PlainSecret::new(api_key.into_bytes()),
        format!("{tenant}/{id}/1").as_bytes(),
    )?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO credentials(id,tenant_id,name,credential_type,masked_hint,owner_department_id,created_by) VALUES(?,?,'M5 LightRAG Fixture Credential','api_key','****',?,?)")
        .bind(id).bind(tenant).bind(department).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO credential_secret_versions(id,tenant_id,credential_id,version_number,algorithm,key_id,nonce,ciphertext,created_by) VALUES(?,?,?,1,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant).bind(id).bind(encrypted.algorithm).bind(encrypted.key_id).bind(encrypted.nonce.to_vec()).bind(encrypted.ciphertext).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

async fn ensure_rag(
    pool: &MySqlPool,
    tenant: Uuid,
    department: Uuid,
    credential: Uuid,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar("SELECT r.id FROM rag_resources r JOIN rag_connections c ON c.id=r.connection_id WHERE r.tenant_id=? AND c.name='M5 LightRAG Fixture' AND r.name='M5 Worker Knowledge'")
        .bind(tenant).fetch_optional(pool).await? {
        return Ok(id);
    }
    let connection = Uuid::now_v7();
    let resource = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO rag_connections(id,tenant_id,name,endpoint,health_path,credential_id,owner_department_id,configuration_json) VALUES(?,?,'M5 LightRAG Fixture','http://lightrag:9621','/health',?,?,?)")
        .bind(connection).bind(tenant).bind(credential).bind(department).bind(json!({"credentialHeader":"X-API-Key"})).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO rag_resources(id,tenant_id,connection_id,name,external_resource_id,owner_department_id,sync_status) VALUES(?,?,?,'M5 Worker Knowledge','m5-worker-fixture',?,'synced')")
        .bind(resource).bind(tenant).bind(connection).bind(department).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(resource)
}

async fn ensure_memory(pool: &MySqlPool, tenant: Uuid, department: Uuid) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar("SELECT n.id FROM memory_namespaces n JOIN memory_connections c ON c.id=n.connection_id WHERE n.tenant_id=? AND c.name='M5 Mem0 Fixture' AND n.name='M5 Worker Memory'")
        .bind(tenant).fetch_optional(pool).await? {
        return Ok(id);
    }
    let connection = Uuid::now_v7();
    let namespace = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO memory_connections(id,tenant_id,name,endpoint,health_path,credential_id,owner_department_id,configuration_json) VALUES(?,?,'M5 Mem0 Fixture','http://mem0:8000','/openapi.json',NULL,?,JSON_OBJECT())")
        .bind(connection).bind(tenant).bind(department).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO memory_namespaces(id,tenant_id,connection_id,name,external_namespace,access_mode,owner_department_id) VALUES(?,?,?,'M5 Worker Memory','m5-worker-fixture','read_write',?)")
        .bind(namespace).bind(tenant).bind(connection).bind(department).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(namespace)
}

async fn ensure_model(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    credential: Uuid,
) -> Result<(Uuid, Uuid)> {
    if let Some(row) = sqlx::query("SELECT a.id,d.id deployment_id FROM model_aliases a JOIN model_deployments d ON d.id=a.deployment_id WHERE a.tenant_id=? AND a.alias='m5-fixture-model'").bind(tenant).fetch_optional(pool).await? {
        return Ok((row.try_get("id")?, row.try_get("deployment_id")?));
    }
    let provider = Uuid::now_v7();
    let deployment = Uuid::now_v7();
    let alias = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO model_providers(id,tenant_id,name,provider_type,endpoint,credential_id,owner_department_id) VALUES(?,?,?,'openai_compatible','http://echo-mcp:8090/v1',?,?)")
        .bind(provider).bind(tenant).bind("M5 OpenAI Fixture").bind(credential).bind(department).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO model_deployments(id,tenant_id,provider_id,name,model_name,credential_id,default_parameters) VALUES(?,?,?,?,?,?,JSON_OBJECT())")
        .bind(deployment).bind(tenant).bind(provider).bind("M5 Deterministic Deployment").bind("echo-model").bind(credential).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO model_aliases(id,tenant_id,alias,deployment_id) VALUES(?,?,'m5-fixture-model',?)").bind(alias).bind(tenant).bind(deployment).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO model_price_versions(id,tenant_id,deployment_id,version_number,currency,input_per_million,output_per_million,created_by) VALUES(?,?,?,1,'USD',1.00000000,2.00000000,?)")
        .bind(Uuid::now_v7()).bind(tenant).bind(deployment).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((alias, deployment))
}

async fn ensure_mcp(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
) -> Result<(Uuid, Uuid, Uuid, Uuid)> {
    if let Some(row) = sqlx::query("SELECT s.id server_id,sv.id server_version_id,t.id tool_id,tv.id tool_version_id FROM mcp_servers s JOIN mcp_server_versions sv ON sv.server_id=s.id AND sv.version_number=s.current_version_number JOIN mcp_tools t ON t.server_id=s.id AND t.name='echo' JOIN mcp_tool_versions tv ON tv.tool_id=t.id AND tv.version_number=t.current_version_number WHERE s.tenant_id=? AND s.name='M5 MCP Fixture'").bind(tenant).fetch_optional(pool).await? {
        return Ok((row.try_get("server_id")?, row.try_get("server_version_id")?, row.try_get("tool_id")?, row.try_get("tool_version_id")?));
    }
    let server = Uuid::now_v7();
    let server_version = Uuid::now_v7();
    let discovery = Uuid::now_v7();
    let tool = Uuid::now_v7();
    let tool_version = Uuid::now_v7();
    let input_schema = json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}},"additionalProperties":false});
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO mcp_servers(id,tenant_id,name,description,owner_department_id,last_discovered_at,created_by) VALUES(?,?,'M5 MCP Fixture','Deterministic Agent tool',?,CURRENT_TIMESTAMP(6),?)").bind(server).bind(tenant).bind(department).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_server_versions(id,tenant_id,server_id,version_number,transport,endpoint,configuration_json,configuration_hash,created_by) VALUES(?,?,?,1,'streamable_http','http://echo-mcp:8090/mcp',JSON_OBJECT(),?,?)")
        .bind(server_version).bind(tenant).bind(server).bind(hash("m5-mcp-server")).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_discovery_runs(id,tenant_id,server_id,server_version_id,status,discovered_count,started_by,finished_at) VALUES(?,?,?,?,'succeeded',1,?,CURRENT_TIMESTAMP(6))")
        .bind(discovery).bind(tenant).bind(server).bind(server_version).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_tools(id,tenant_id,server_id,name,title,description,current_version_number) VALUES(?,?,?,'echo','Echo','Return text unchanged',1)").bind(tool).bind(tenant).bind(server).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_tool_versions(id,tenant_id,tool_id,discovery_run_id,version_number,input_schema,output_schema,annotations_json,schema_hash) VALUES(?,?,?,?,1,?,?,JSON_OBJECT(),?)")
        .bind(tool_version).bind(tenant).bind(tool).bind(discovery).bind(&input_schema).bind(json!({"type":"object","properties":{"text":{"type":"string"}}})).bind(hash(&input_schema.to_string())).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mcp_tool_policies(tenant_id,tool_id,enabled,debug_enabled,timeout_seconds,side_effect,updated_by) VALUES(?,?,TRUE,TRUE,30,'read_only',?)").bind(tenant).bind(tool).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((server, server_version, tool, tool_version))
}

#[allow(clippy::too_many_arguments)]
async fn ensure_sandbox_profile(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    image: &str,
    runner: &str,
    name: &str,
    limits: SandboxLimits,
) -> Result<(Uuid, Uuid)> {
    if let Some(row) = sqlx::query("SELECT p.id,v.id version_id FROM sandbox_profiles p JOIN sandbox_profile_versions v ON v.profile_id=p.id AND v.version_number=p.current_version_number WHERE p.tenant_id=? AND p.name=?").bind(tenant).bind(name).fetch_optional(pool).await? {
        return Ok((row.try_get("id")?, row.try_get("version_id")?));
    }
    let profile = Uuid::now_v7();
    let version = Uuid::now_v7();
    let policy = json!({"defaultAction":"deny","egress":[]});
    let configuration = json!({"runner":runner,"imageDigest":image,"cpuMillis":limits.cpu_millis,"memoryBytes":limits.memory_bytes,"pidsLimit":limits.pids_limit,"diskBytes":limits.disk_bytes,"timeoutSeconds":limits.timeout_seconds,"outputLimitBytes":limits.output_limit_bytes,"networkPolicy":policy});
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO sandbox_profiles(id,tenant_id,name,description,owner_department_id,created_by) VALUES(?,?,?,'Pinned OpenSandbox command runner',?,?)").bind(profile).bind(tenant).bind(name).bind(department).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO sandbox_profile_versions(id,tenant_id,profile_id,version_number,runner,image_digest,cpu_millis,memory_bytes,pids_limit,disk_bytes,timeout_seconds,output_limit_bytes,network_policy_json,configuration_hash,created_by) VALUES(?,?,?,1,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(version).bind(tenant).bind(profile).bind(runner).bind(image).bind(limits.cpu_millis).bind(limits.memory_bytes).bind(limits.pids_limit).bind(limits.disk_bytes).bind(limits.timeout_seconds).bind(limits.output_limit_bytes).bind(&policy).bind(hash(&configuration.to_string())).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((profile, version))
}

async fn create_agent_workflow(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    resources: &Resources,
    loop_fixture: bool,
) -> Result<Uuid> {
    let name = if loop_fixture {
        "M5 Agent Loop Fixture"
    } else {
        "M5 Agent MCP Fixture"
    };
    let prompt = if loop_fixture {
        "m5-loop: repeat the same tool"
    } else {
        "Call the echo tool once and finish"
    };
    let definition = json!({
        "schemaVersion":"3.0",
        "nodes":[
            node("trigger","manual_trigger","Manual Trigger",80,160,json!({})),
            node_with_resources("agent","agent","M5 Agent",360,160,json!({"systemPrompt":"Use the authorized tools only.","messages":[{"role":"user","content":prompt}],"maxIterations":6,"maxModelCalls":6,"maxToolCalls":8,"maxTotalTokens":4096,"maxOutputTokens":512,"maxCostMicros":1000000,"maxDurationMs":120000,"limitAction":"error_output"}),vec![
                binding_reference("agent-model","ai_model","model",resources.model,Some(resources.model_version),"use"),
                binding_reference("agent-tool","ai_tool","mcp_tool",resources.mcp_tool,Some(resources.mcp_tool_version),"use")
            ])
        ],
        "connections":[edge("agent-start","trigger","main","agent","main")]
    });
    let snapshots = agent_snapshots(resources);
    create_workflow(pool, tenant, user, department, name, definition, snapshots).await
}

async fn create_knowledge_workflows(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    resources: &Resources,
) -> Result<()> {
    let rag_query = json!({
        "schemaVersion":"3.0",
        "nodes":[
            node("trigger","manual_trigger","Manual Trigger",80,160,json!({})),
            node_with_resources("rag","rag","M5 RAG Query",360,160,json!({
                "operation":"query",
                "input":{"query":"Which adapter does Agentx M5 use?","mode":"naive","include_references":true}
            }),vec![reference("rag",resources.rag,None,"read")])
        ],
        "connections":[edge("rag-start","trigger","main","rag","main")]
    });
    create_workflow(
        pool,
        tenant,
        user,
        department,
        "M5 RAG Query Fixture",
        rag_query,
        rag_snapshots(resources, "read"),
    )
    .await?;

    let memory_search = json!({
        "schemaVersion":"3.0",
        "nodes":[
            node("trigger","manual_trigger","Manual Trigger",80,160,json!({})),
            node_with_resources("memory","memory","M5 Memory Search",360,160,json!({
                "operation":"search",
                "input":{"query":"Which adapter does Agentx M5 use?","top_k":5}
            }),vec![reference("memory",resources.memory,None,"read")])
        ],
        "connections":[edge("memory-start","trigger","main","memory","main")]
    });
    create_workflow(
        pool,
        tenant,
        user,
        department,
        "M5 Memory Search Fixture",
        memory_search,
        memory_snapshots(resources, "read"),
    )
    .await?;

    let denied_write = json!({
        "schemaVersion":"3.0",
        "nodes":[
            node("trigger","manual_trigger","Manual Trigger",80,160,json!({})),
            node_with_resources("rag","rag","M5 RAG Read Scope",360,160,json!({
                "operation":"insert",
                "input":{"text":"This write must never reach LightRAG.","file_source":"m5-denied-write.txt"}
            }),vec![reference("rag",resources.rag,None,"read")])
        ],
        "connections":[edge("rag-denied-start","trigger","main","rag","main")]
    });
    create_workflow(
        pool,
        tenant,
        user,
        department,
        "M5 RAG Read Scope Fixture",
        denied_write,
        rag_snapshots(resources, "read"),
    )
    .await?;
    Ok(())
}

fn rag_snapshots(resources: &Resources, operation: &'static str) -> Vec<Snapshot> {
    vec![
        Snapshot {
            node_id: "rag",
            resource_type: "rag",
            resource_id: resources.rag,
            resource_version_id: None,
            operation,
            value: json!({
                "resourceId":resources.rag,
                "externalResourceId":"m5-worker-fixture",
                "resourceVersion":1,
                "connectionId":Uuid::nil(),
                "endpoint":"http://lightrag:9621",
                "connectionVersion":1,
                "credentialId":resources.rag_credential,
                "configuration":{"credentialHeader":"X-API-Key"}
            }),
        },
        Snapshot {
            node_id: "rag",
            resource_type: "credential",
            resource_id: resources.rag_credential,
            resource_version_id: None,
            operation: "use",
            value: json!({"id":resources.rag_credential,"credentialType":"api_key","secretVersion":1,"resourceVersion":1}),
        },
    ]
}

fn memory_snapshots(resources: &Resources, operation: &'static str) -> Vec<Snapshot> {
    vec![Snapshot {
        node_id: "memory",
        resource_type: "memory",
        resource_id: resources.memory,
        resource_version_id: None,
        operation,
        value: json!({
            "namespaceId":resources.memory,
            "externalNamespace":"m5-worker-fixture",
            "accessMode":"read_write",
            "resourceVersion":1,
            "connectionId":Uuid::nil(),
            "endpoint":"http://mem0:8000",
            "connectionVersion":1,
            "credentialId":null,
            "configuration":{}
        }),
    }]
}

async fn create_code_workflow(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    resources: &Resources,
) -> Result<Uuid> {
    create_code_fixture(
        pool,
        tenant,
        user,
        department,
        resources,
        CodeFixture {
            workflow_name: "M5 Code Fixture",
            node_name: "M5 Python Code",
            runner: "python",
            source: "from pathlib import Path\nprint('m5-sandbox-ok')\nPath('/workspace/result.txt').write_text('m5-artifact-ok', encoding='utf-8')",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: Some("/workspace/result.txt"),
            profile: resources.sandbox_profile,
            profile_version: resources.sandbox_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await
}

struct CodeFixture {
    workflow_name: &'static str,
    node_name: &'static str,
    runner: &'static str,
    source: &'static str,
    image: String,
    limits: SandboxLimits,
    expected_output_path: Option<&'static str>,
    profile: Uuid,
    profile_version: Uuid,
    network_policy: Option<Value>,
    credential: bool,
}

async fn create_code_fixture(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    resources: &Resources,
    fixture: CodeFixture,
) -> Result<Uuid> {
    let mut parameters = json!({"runner":fixture.runner,"source":fixture.source});
    if let Some(path) = fixture.expected_output_path {
        parameters["outputPaths"] = json!([path]);
    }
    if let Some(policy) = fixture.network_policy.clone() {
        parameters["networkPolicy"] = policy;
    }
    if fixture.credential {
        parameters["credentialFiles"] = json!({"M5_SECRET_FILE":resources.credential});
    }
    let mut references = vec![reference(
        "sandbox_profile",
        fixture.profile,
        Some(fixture.profile_version),
        "use",
    )];
    if fixture.credential {
        references.push(reference("credential", resources.credential, None, "use"));
    }
    let definition = json!({
        "schemaVersion":"3.0",
        "nodes":[
            node("trigger","manual_trigger","Manual Trigger",80,160,json!({})),
            node_with_resources("code","code",fixture.node_name,360,160,parameters,references)
        ],
        "connections":[edge("code-start","trigger","main","code","main")]
    });
    let limits = fixture.limits;
    let configuration = json!({"runner":fixture.runner,"imageDigest":fixture.image,"cpuMillis":limits.cpu_millis,"memoryBytes":limits.memory_bytes,"pidsLimit":limits.pids_limit,"diskBytes":limits.disk_bytes,"timeoutSeconds":limits.timeout_seconds,"outputLimitBytes":limits.output_limit_bytes,"networkPolicy":{"defaultAction":"deny","egress":[]}});
    let mut snapshots = vec![Snapshot {
        node_id: "code",
        resource_type: "sandbox_profile",
        resource_id: fixture.profile,
        resource_version_id: Some(fixture.profile_version),
        operation: "use",
        value: json!({"profileVersionId":fixture.profile_version,"versionNumber":1,"runner":fixture.runner,"imageDigest":fixture.image,"cpuMillis":limits.cpu_millis,"memoryBytes":limits.memory_bytes,"pidsLimit":limits.pids_limit,"diskBytes":limits.disk_bytes,"timeoutSeconds":limits.timeout_seconds,"outputLimitBytes":limits.output_limit_bytes,"networkPolicy":{"defaultAction":"deny","egress":[]},"configurationHash":hash(&configuration.to_string())}),
    }];
    if fixture.credential {
        snapshots.push(Snapshot {
            node_id: "code",
            resource_type: "credential",
            resource_id: resources.credential,
            resource_version_id: None,
            operation: "use",
            value: json!({"id":resources.credential,"credentialType":"bearer","secretVersion":1,"resourceVersion":1}),
        });
    }
    create_workflow(
        pool,
        tenant,
        user,
        department,
        fixture.workflow_name,
        definition,
        snapshots,
    )
    .await
}

async fn create_network_workflows(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    resources: &Resources,
) -> Result<()> {
    create_code_fixture(
        pool,
        tenant,
        user,
        department,
        resources,
        CodeFixture {
            workflow_name: "M5 Network Deny Fixture",
            node_name: "M5 Network Deny",
            runner: "python",
            source: "import socket\nfor family, label in [(socket.AF_INET, 'ipv4'), (socket.AF_INET6, 'ipv6')]:\n    try:\n        socket.getaddrinfo('one.one.one.one', 443, family, socket.SOCK_STREAM)\n    except Exception:\n        print(f'm5-{label}-denied')\n    else:\n        raise RuntimeError(f'{label} DNS unexpectedly allowed')\nprint('m5-network-denied')",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: None,
            profile: resources.sandbox_profile,
            profile_version: resources.sandbox_profile_version,
            network_policy: None,
            credential: false,
        },
    )
    .await?;
    create_code_fixture(
        pool,
        tenant,
        user,
        department,
        resources,
        CodeFixture {
            workflow_name: "M5 Network Allow Fixture",
            node_name: "M5 Network Allow",
            runner: "python",
            source: "import urllib.request\nwith urllib.request.urlopen('https://example.com', timeout=10) as response:\n    assert response.status == 200\nprint('m5-network-allowed')",
            image: resources.sandbox_image.clone(),
            limits: SandboxLimits::STANDARD,
            expected_output_path: None,
            profile: resources.sandbox_profile,
            profile_version: resources.sandbox_profile_version,
            network_policy: Some(json!({"defaultAction":"deny","egress":[{"action":"allow","target":"example.com"}]})),
            credential: false,
        },
    )
    .await?;
    Ok(())
}

fn agent_snapshots(resources: &Resources) -> Vec<Snapshot> {
    vec![
        Snapshot {
            node_id: "agent",
            resource_type: "model",
            resource_id: resources.model,
            resource_version_id: Some(resources.model_version),
            operation: "use",
            value: json!({"aliasId":resources.model,"alias":"m5-fixture-model","aliasVersion":1,"deploymentId":resources.model_version,"deploymentVersion":1,"modelName":"echo-model","defaultParameters":{},"providerId":Uuid::nil(),"providerType":"openai_compatible","endpoint":"http://echo-mcp:8090/v1","providerVersion":1,"credentialId":resources.credential,"price":{"versionId":Uuid::nil(),"versionNumber":1,"currency":"USD","inputPerMillion":"1.00000000","outputPerMillion":"2.00000000"}}),
        },
        Snapshot {
            node_id: "agent",
            resource_type: "credential",
            resource_id: resources.credential,
            resource_version_id: None,
            operation: "use",
            value: json!({"id":resources.credential,"credentialType":"bearer","secretVersion":1,"resourceVersion":1}),
        },
        Snapshot {
            node_id: "agent",
            resource_type: "mcp_tool",
            resource_id: resources.mcp_tool,
            resource_version_id: Some(resources.mcp_tool_version),
            operation: "use",
            value: json!({"toolVersionId":resources.mcp_tool_version,"toolVersionNumber":1,"toolName":"echo","title":"Echo","serverId":resources.mcp_server,"serverVersionId":resources.mcp_server_version,"transport":"streamable_http","endpoint":"http://echo-mcp:8090/mcp","credentialId":null,"configurationHash":hash("m5-mcp-server"),"inputSchema":{"type":"object","required":["text"],"properties":{"text":{"type":"string"}},"additionalProperties":false},"outputSchema":{"type":"object","properties":{"text":{"type":"string"}}},"annotations":{},"schemaHash":hash("m5-echo-schema"),"timeoutSeconds":30,"sideEffect":"read_only"}),
        },
        Snapshot {
            node_id: "agent",
            resource_type: "mcp_server",
            resource_id: resources.mcp_server,
            resource_version_id: Some(resources.mcp_server_version),
            operation: "use",
            value: json!({"serverId":resources.mcp_server,"name":"M5 MCP Fixture","serverVersionId":resources.mcp_server_version,"versionNumber":1,"transport":"streamable_http","endpoint":"http://echo-mcp:8090/mcp","credentialId":null,"configurationHash":hash("m5-mcp-server")}),
        },
    ]
}

async fn create_workflow(
    pool: &MySqlPool,
    tenant: Uuid,
    user: Uuid,
    department: Uuid,
    name: &str,
    definition: Value,
    snapshots: Vec<Snapshot>,
) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT v.id FROM workflow_versions v JOIN workflows w ON w.id=v.workflow_id AND w.tenant_id=v.tenant_id WHERE v.tenant_id=? AND w.name=? ORDER BY v.version_number DESC LIMIT 1").bind(tenant).bind(name).fetch_optional(pool).await? { return Ok(id); }
    let workflow = Uuid::now_v7();
    let draft = Uuid::now_v7();
    let version = Uuid::now_v7();
    let identity = Uuid::now_v7();
    let parsed: WorkflowDefinition = serde_json::from_value(definition.clone())?;
    let compiled = WorkflowCompiler::new(&NodeRegistry::m5_defaults())
        .compile(
            &parsed,
            &CompileContext {
                current_workflow_version_id: Some(version.to_string()),
                ancestor_workflow_version_ids: Default::default(),
            },
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{name}: {}",
                serde_json::to_string(&error.issues).unwrap_or_else(|_| error.to_string())
            )
        })?;
    let content_hash = canonical_content_hash(&definition)?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO workflows(id,tenant_id,name,description,visibility,owner_user_id,owner_department_id) VALUES(?,?,?,'M5 Kubernetes E2E fixture','company',?,?)").bind(workflow).bind(tenant).bind(name).bind(user).bind(department).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_service_identities(id,tenant_id,workflow_id) VALUES(?,?,?)")
        .bind(identity)
        .bind(tenant)
        .bind(workflow)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workflow_members(tenant_id,workflow_id,user_id,member_role,created_by) VALUES(?,?,?,'manager',?)").bind(tenant).bind(workflow).bind(user).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_drafts(id,tenant_id,workflow_id,schema_version,revision,definition_json,content_hash,updated_by) VALUES(?,?,?,'3.0',1,?,?,?)").bind(draft).bind(tenant).bind(workflow).bind(&definition).bind(&content_hash).bind(user).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_versions(id,tenant_id,workflow_id,version_number,source_revision,schema_version,definition_json,content_hash,compiled_ir_json,compiled_ir_hash,compiler_version,compiled_at,created_by) VALUES(?,?,?,1,1,'3.0',?,?,?,?,?,CURRENT_TIMESTAMP(6),?)").bind(version).bind(tenant).bind(workflow).bind(&definition).bind(&content_hash).bind(serde_json::to_value(&compiled)?).bind(&compiled.canonical_hash).bind(&compiled.compiler_version).bind(user).execute(&mut *tx).await?;
    for snapshot in snapshots {
        insert_snapshot_and_grant(&mut tx, tenant, user, identity, version, snapshot).await?;
    }
    tx.commit().await?;
    Ok(version)
}

async fn insert_snapshot_and_grant(
    tx: &mut Transaction<'_, MySql>,
    tenant: Uuid,
    user: Uuid,
    identity: Uuid,
    workflow_version: Uuid,
    snapshot: Snapshot,
) -> Result<()> {
    let snapshot_hash = canonical_content_hash(&snapshot.value)?;
    sqlx::query("INSERT INTO workflow_version_resources(id,tenant_id,workflow_version_id,node_id,resource_type,resource_id,resource_version_id,operation_key,snapshot_json,snapshot_hash) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant).bind(workflow_version).bind(snapshot.node_id).bind(snapshot.resource_type).bind(snapshot.resource_id).bind(snapshot.resource_version_id).bind(snapshot.operation).bind(snapshot.value).bind(snapshot_hash).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO resource_grants(id,tenant_id,subject_type,subject_id,resource_type,resource_id,resource_version_id,operation_key,created_by) VALUES(?,?,'workflow_service_identity',?,?,?,?,?,?)")
        .bind(Uuid::now_v7()).bind(tenant).bind(identity).bind(snapshot.resource_type).bind(snapshot.resource_id).bind(snapshot.resource_version_id).bind(snapshot.operation).bind(user).execute(&mut **tx).await?;
    Ok(())
}

fn reference(
    resource_type: &str,
    resource_id: Uuid,
    resource_version_id: Option<Uuid>,
    operation: &str,
) -> Value {
    json!({"resourceType":resource_type,"resourceId":resource_id,"resourceVersionId":resource_version_id,"operation":operation})
}
fn binding_reference(
    binding_id: &str,
    binding_role: &str,
    resource_type: &str,
    resource_id: Uuid,
    resource_version_id: Option<Uuid>,
    operation: &str,
) -> Value {
    json!({"bindingId":binding_id,"bindingRole":binding_role,"resourceType":resource_type,"resourceId":resource_id,"resourceVersionId":resource_version_id,"operation":operation})
}
fn node(id: &str, node_type: &str, name: &str, _x: i32, _y: i32, parameters: Value) -> Value {
    json!({"id":id,"type":node_type,"typeVersion":1,"name":name,"parameters":parameters})
}
fn node_with_resources(
    id: &str,
    node_type: &str,
    name: &str,
    _x: i32,
    _y: i32,
    parameters: Value,
    resources: Vec<Value>,
) -> Value {
    json!({"id":id,"type":node_type,"typeVersion":1,"name":name,"parameters":parameters,"resourceReferences":resources})
}
fn edge(id: &str, source: &str, source_handle: &str, target: &str, target_handle: &str) -> Value {
    json!({"id":id,"sourceNodeId":source,"sourceHandle":source_handle,"targetNodeId":target,"targetHandle":target_handle,"order":0})
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
