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
        })?
    );
    Ok(())
}
