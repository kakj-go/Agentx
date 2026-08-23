use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey as _};
use rand::rngs::OsRng;
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey as _, EncodePublicKey as _, LineEnding},
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningMaterial {
    pub service_private_key_pem: String,
    pub service_public_key_pem: String,
    pub projector_private_key_pem: String,
    pub projector_public_key_pem: String,
    pub bff_private_key_pem: String,
    pub bff_public_key_pem: String,
    pub bundle_private_key_pem: String,
    pub bundle_public_key_base64: String,
    pub work_package_private_key_pem: String,
    pub work_package_public_key_base64: String,
    pub user_private_key_pem: String,
    pub user_public_key_pem: String,
    pub runtime_gateway_egress_private_key_pem: String,
    pub runtime_gateway_egress_public_key_pem: String,
    pub workflow_runtime_egress_private_key_pem: String,
    pub workflow_runtime_egress_public_key_pem: String,
    pub workflow_worker_egress_private_key_pem: String,
    pub workflow_worker_egress_public_key_pem: String,
    pub sandbox_egress_private_key_pem: String,
    pub sandbox_egress_public_key_pem: String,
    pub egress_tls_certificate_pem: String,
    pub egress_tls_private_key_pem: String,
}

pub fn rsa_pair() -> Result<(String, String)> {
    let private = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let public = RsaPublicKey::from(&private);
    Ok((
        private.to_pkcs8_pem(LineEnding::LF)?.to_string(),
        public.to_public_key_pem(LineEnding::LF)?,
    ))
}

pub fn rsa_public_key_pem(private_key_pem: &str) -> Result<String> {
    let private = RsaPrivateKey::from_pkcs8_pem(private_key_pem)?;
    Ok(RsaPublicKey::from(&private).to_public_key_pem(LineEnding::LF)?)
}

pub fn ed25519_pair() -> Result<(String, String)> {
    let private = SigningKey::generate(&mut OsRng);
    Ok((
        private.to_pkcs8_pem(LineEnding::LF)?.to_string(),
        STANDARD.encode(private.verifying_key().as_bytes()),
    ))
}

pub fn tls_pair() -> Result<(String, String)> {
    let certificate = rcgen::generate_simple_self_signed(vec!["agentx-egress-gateway".into()])?;
    Ok((certificate.key_pair.serialize_pem(), certificate.cert.pem()))
}

pub fn generate() -> Result<SigningMaterial> {
    let service = rsa_pair()?;
    let projector = rsa_pair()?;
    let bff = rsa_pair()?;
    let user = rsa_pair()?;
    let bundle = ed25519_pair()?;
    let work_package = ed25519_pair()?;
    let runtime_gateway = rsa_pair()?;
    let workflow_runtime = rsa_pair()?;
    let workflow_worker = rsa_pair()?;
    let sandbox = rsa_pair()?;
    let tls = tls_pair()?;
    Ok(SigningMaterial {
        service_private_key_pem: service.0,
        service_public_key_pem: service.1,
        projector_private_key_pem: projector.0,
        projector_public_key_pem: projector.1,
        bff_private_key_pem: bff.0,
        bff_public_key_pem: bff.1,
        bundle_private_key_pem: bundle.0,
        bundle_public_key_base64: bundle.1,
        work_package_private_key_pem: work_package.0,
        work_package_public_key_base64: work_package.1,
        user_private_key_pem: user.0,
        user_public_key_pem: user.1,
        runtime_gateway_egress_private_key_pem: runtime_gateway.0,
        runtime_gateway_egress_public_key_pem: runtime_gateway.1,
        workflow_runtime_egress_private_key_pem: workflow_runtime.0,
        workflow_runtime_egress_public_key_pem: workflow_runtime.1,
        workflow_worker_egress_private_key_pem: workflow_worker.0,
        workflow_worker_egress_public_key_pem: workflow_worker.1,
        sandbox_egress_private_key_pem: sandbox.0,
        sandbox_egress_public_key_pem: sandbox.1,
        egress_tls_certificate_pem: tls.1,
        egress_tls_private_key_pem: tls.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_material_has_expected_encodings() {
        let material = generate().unwrap();
        assert!(
            material
                .service_private_key_pem
                .contains("BEGIN PRIVATE KEY")
        );
        assert!(material.service_public_key_pem.contains("BEGIN PUBLIC KEY"));
        assert_eq!(
            rsa_public_key_pem(&material.service_private_key_pem).unwrap(),
            material.service_public_key_pem
        );
        assert_eq!(
            STANDARD
                .decode(material.bundle_public_key_base64)
                .unwrap()
                .len(),
            32
        );
        assert!(
            material
                .egress_tls_certificate_pem
                .contains("BEGIN CERTIFICATE")
        );
    }
}
