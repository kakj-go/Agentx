use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey as _};
use rand::rngs::OsRng;
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{EncodePublicKey as _, LineEnding},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SigningMaterial {
    service_private_key_pem: String,
    service_public_key_pem: String,
    projector_private_key_pem: String,
    projector_public_key_pem: String,
    bff_private_key_pem: String,
    bff_public_key_pem: String,
    bundle_private_key_pem: String,
    bundle_public_key_base64: String,
    work_package_private_key_pem: String,
    work_package_public_key_base64: String,
    user_private_key_pem: String,
    user_public_key_pem: String,
    runtime_gateway_egress_private_key_pem: String,
    runtime_gateway_egress_public_key_pem: String,
    workflow_runtime_egress_private_key_pem: String,
    workflow_runtime_egress_public_key_pem: String,
    workflow_worker_egress_private_key_pem: String,
    workflow_worker_egress_public_key_pem: String,
    sandbox_egress_private_key_pem: String,
    sandbox_egress_public_key_pem: String,
    egress_tls_certificate_pem: String,
    egress_tls_private_key_pem: String,
}

fn main() -> Result<()> {
    let service = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let service_public = RsaPublicKey::from(&service);
    let projector = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let projector_public = RsaPublicKey::from(&projector);
    let bff = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let bff_public = RsaPublicKey::from(&bff);
    let bundle = SigningKey::generate(&mut OsRng);
    let work_package = SigningKey::generate(&mut OsRng);
    let user = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let user_public = RsaPublicKey::from(&user);
    let runtime_gateway_egress = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let workflow_runtime_egress = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let workflow_worker_egress = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let sandbox_egress = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let egress_tls = rcgen::generate_simple_self_signed(vec![
        "agentx-egress-sandbox".into(),
        "agentx-egress-sandbox.agentx-v2-deps.svc".into(),
        "agentx-egress-sandbox.agentx-v2-deps.svc.cluster.local".into(),
        "host.docker.internal".into(),
    ])?;
    println!(
        "{}",
        serde_json::to_string(&SigningMaterial {
            service_private_key_pem: service.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            service_public_key_pem: service_public.to_public_key_pem(LineEnding::LF)?,
            projector_private_key_pem: projector.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            projector_public_key_pem: projector_public.to_public_key_pem(LineEnding::LF)?,
            bff_private_key_pem: bff.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            bff_public_key_pem: bff_public.to_public_key_pem(LineEnding::LF)?,
            bundle_private_key_pem: bundle.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            bundle_public_key_base64: STANDARD.encode(bundle.verifying_key().as_bytes()),
            work_package_private_key_pem: work_package.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            work_package_public_key_base64: STANDARD
                .encode(work_package.verifying_key().as_bytes()),
            user_private_key_pem: user.to_pkcs8_pem(LineEnding::LF)?.to_string(),
            user_public_key_pem: user_public.to_public_key_pem(LineEnding::LF)?,
            runtime_gateway_egress_private_key_pem: runtime_gateway_egress
                .to_pkcs8_pem(LineEnding::LF)?
                .to_string(),
            runtime_gateway_egress_public_key_pem: RsaPublicKey::from(&runtime_gateway_egress)
                .to_public_key_pem(LineEnding::LF)?,
            workflow_runtime_egress_private_key_pem: workflow_runtime_egress
                .to_pkcs8_pem(LineEnding::LF)?
                .to_string(),
            workflow_runtime_egress_public_key_pem: RsaPublicKey::from(&workflow_runtime_egress)
                .to_public_key_pem(LineEnding::LF)?,
            workflow_worker_egress_private_key_pem: workflow_worker_egress
                .to_pkcs8_pem(LineEnding::LF)?
                .to_string(),
            workflow_worker_egress_public_key_pem: RsaPublicKey::from(&workflow_worker_egress)
                .to_public_key_pem(LineEnding::LF)?,
            sandbox_egress_private_key_pem: sandbox_egress
                .to_pkcs8_pem(LineEnding::LF)?
                .to_string(),
            sandbox_egress_public_key_pem: RsaPublicKey::from(&sandbox_egress)
                .to_public_key_pem(LineEnding::LF)?,
            egress_tls_certificate_pem: egress_tls.cert.pem(),
            egress_tls_private_key_pem: egress_tls.key_pair.serialize_pem(),
        })?
    );
    Ok(())
}
