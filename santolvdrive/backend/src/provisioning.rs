use crate::{installer::write_new, permissions};
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
struct Material {
    host: String,
    ca_certificate: String,
    ca_key: String,
    certificate: String,
    key: String,
}

pub fn tls_material(root: &Path, origin: &str) -> Result<(PathBuf, PathBuf)> {
    let u = url::Url::parse(origin)?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.origin().ascii_serialization() != origin
    {
        return Err("a canonical HTTPS origin is required".into());
    }
    let host = u
        .host_str()
        .ok_or("HTTPS origin requires hostname")?
        .trim_matches(['[', ']'])
        .to_owned();
    permissions::private_directory(root)?;
    let state = root.join("tls-state.json");
    let material: Material = if state.exists() {
        serde_json::from_slice(&fs::read(&state)?)?
    } else {
        let now = time::OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(DnType::CommonName, "SantolvDrive local CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let permitted = match host.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(ip)) => {
                GeneralSubtree::IpAddress(CidrSubnet::V4(ip.octets(), [255; 4]))
            }
            Ok(std::net::IpAddr::V6(ip)) => {
                GeneralSubtree::IpAddress(CidrSubnet::V6(ip.octets(), [255; 16]))
            }
            Err(_) => GeneralSubtree::DnsName(host.clone()),
        };
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![permitted],
            excluded_subtrees: vec![],
        });
        params.not_before = now - time::Duration::minutes(5);
        params.not_after = now + time::Duration::days(3650);
        let ca = params.self_signed(&ca_key)?;
        let issuer = Issuer::new(params, ca_key);
        let key = KeyPair::generate()?;
        let mut leaf = CertificateParams::new(vec![host.clone()])?;
        leaf.distinguished_name
            .push(DnType::CommonName, host.clone());
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        // shortcut: local leaves expire after one year; add in-process renewal before unattended long-term deployments.
        leaf.not_before = now - time::Duration::minutes(5);
        leaf.not_after = now + time::Duration::days(365);
        let certificate = leaf.signed_by(&key, &issuer)?;
        let material = Material {
            host: host.clone(),
            ca_certificate: ca.pem(),
            ca_key: issuer.key().serialize_pem(),
            certificate: certificate.pem(),
            key: key.serialize_pem(),
        };
        write_new(&state, &serde_json::to_vec_pretty(&material)?)?;
        material
    };
    if material.host != host {
        return Err("existing TLS identity preserved; change domain through an explicit certificate migration".into());
    }
    let cert = root.join("certificate.pem");
    let key = root.join("private-key.pem");
    write_new(&cert, material.certificate.as_bytes())?;
    write_new(&key, material.key.as_bytes())?;
    write_new(
        &root.join("local-ca.pem"),
        material.ca_certificate.as_bytes(),
    )?;
    crate::tls::config(&cert, &key)?;
    Ok((cert, key))
}
#[cfg(test)]
mod tests {
    #[test]
    fn local_ca_validates_domain_and_rejects_other_names() {
        use tokio_rustls::rustls::{
            self,
            client::danger::ServerCertVerifier,
            pki_types::{CertificateDer, UnixTime, pem::PemObject},
        };
        let t = tempfile::tempdir().unwrap();
        let (cert, _) = super::tls_material(t.path(), "https://localhost:8443").unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_file(t.path().join("local-ca.pem")).unwrap())
            .unwrap();
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            std::sync::Arc::new(roots),
            std::sync::Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .unwrap();
        let cert = CertificateDer::from_pem_file(cert).unwrap();
        assert!(
            verifier
                .verify_server_cert(
                    &cert,
                    &[],
                    &"localhost".try_into().unwrap(),
                    &[],
                    UnixTime::now()
                )
                .is_ok()
        );
        assert!(
            verifier
                .verify_server_cert(
                    &cert,
                    &[],
                    &"other.example.com".try_into().unwrap(),
                    &[],
                    UnixTime::now()
                )
                .is_err()
        );
        // Even a certificate signed with this CA must not authenticate another domain.
        let material: super::Material =
            serde_json::from_slice(&std::fs::read(t.path().join("tls-state.json")).unwrap())
                .unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "SantolvDrive local CA");
        params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
        let issuer =
            rcgen::Issuer::new(params, rcgen::KeyPair::from_pem(&material.ca_key).unwrap());
        let key = rcgen::KeyPair::generate().unwrap();
        let unauthorized = rcgen::CertificateParams::new(vec!["other.example.com".into()])
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        assert!(
            verifier
                .verify_server_cert(
                    unauthorized.der(),
                    &[],
                    &"other.example.com".try_into().unwrap(),
                    &[],
                    UnixTime::now()
                )
                .is_err()
        );
    }
    #[test]
    fn generated_identity_is_valid_and_stable() {
        let t = tempfile::tempdir().unwrap();
        let (c, k) = super::tls_material(t.path(), "https://localhost:8443").unwrap();
        let cert = std::fs::read(&c).unwrap();
        let key = std::fs::read(&k).unwrap();
        super::tls_material(t.path(), "https://localhost:8443").unwrap();
        assert_eq!(std::fs::read(c).unwrap(), cert);
        assert_eq!(std::fs::read(k).unwrap(), key);
        assert!(super::tls_material(t.path(), "https://other.example.com").is_err());
        assert!(t.path().join("local-ca.pem").exists());
    }
}
