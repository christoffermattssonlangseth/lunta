//! The certificates behind https://localhost:47481 on Windows and Linux, as Lunta for Mac makes
//! them (`apple/LuntaCore/Sources/LuntaCore/CitationCertificates.swift`): a certificate authority
//! made on this computer (trusted once, with Citations in Word & Google Docs… › Trust Certificate)
//! and the localhost certificate it signs.
//!
//! The authority may only vouch for localhost and 127.0.0.1 (name constraints, critical), so its
//! key is kept for the renewals: the localhost certificate (820 days, what macOS accepts and more
//! than enough elsewhere) is renewed a month before it runs out without trusting anything again.
//! A new authority is made when there is no key to sign with, or the next certificate would
//! outlive it; that one must be trusted again.
//!
//! Files in `dir` (`<app data>/citations/tls`): `ca.pem`, `ca.key`, `server.pem`, `server.key`.
//! The keys are protected for this user: with DPAPI on Windows (the file is useless on another
//! account or computer), readable by this user only (0600, folder 0700) elsewhere.
//! ECDSA P-256 keys (ring can't make RSA keys); Schannel, NSS and every browser take them.

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, NameConstraints, SanType, SerialNumber,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::path::Path;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The certificate is good for at least another month.
    Current,
    /// A new localhost certificate from the same authority: still trusted.
    Renewed,
    /// A new authority: it must be trusted (Trust Certificate) before the add-ins connect.
    NewAuthority,
    Failed(String),
}

pub const RENEW_BEFORE_DAYS: i64 = 30;
pub const LEAF_DAYS: i64 = 820;
pub const CA_DAYS: i64 = 3650;
/// The authority's name, as Windows shows it when asked to trust it, and in certificate lists.
pub const CA_NAME: &str = "Lunta Local CA (this computer only, localhost)";

/// Makes or renews the certificates in `dir` as needed.
pub fn ensure(dir: &Path, now: OffsetDateTime) -> Outcome {
    match ensure_inner(dir, now) {
        Ok(o) => o,
        Err(e) => Outcome::Failed(e),
    }
}

fn ensure_inner(dir: &Path, now: OffsetDateTime) -> Result<Outcome, String> {
    let left = |name: &str| expiry(&dir.join(name)).map(|t| t - now).unwrap_or(Duration::ZERO);
    let renew_before = Duration::days(RENEW_BEFORE_DAYS);
    let leaf_ok = dir.join("server.key").exists() && read_key(&dir.join("server.key")).is_ok();
    if leaf_ok && left("server.pem") > renew_before && left("ca.pem") > renew_before && leaf_from_authority(dir) {
        return Ok(Outcome::Current);
    }
    make_private_dir(dir)?;
    let mut new_authority = false;
    let ca_key = match read_key(&dir.join("ca.key")) {
        // a new authority when there is no key to sign with, or the next certificate would outlive it
        Ok(k) if left("ca.pem") >= Duration::days(LEAF_DAYS + 10) => k,
        _ => {
            let key = KeyPair::generate().map_err(|e| format!("making the authority's key: {e}"))?;
            let mut p = CertificateParams::default();
            p.distinguished_name = name(CA_NAME);
            p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
            p.name_constraints = Some(NameConstraints {
                permitted_subtrees: vec![GeneralSubtree::DnsName("localhost".into()), GeneralSubtree::IpAddress(CidrSubnet::V4([127, 0, 0, 1], [255; 4]))],
                excluded_subtrees: vec![],
            });
            p.not_before = now - Duration::days(1);
            p.not_after = now + Duration::days(CA_DAYS);
            p.serial_number = Some(serial());
            let cert = p.self_signed(&key).map_err(|e| format!("making the authority: {e}"))?;
            // the key first: an authority whose key is lost is replaced at the next start
            write_key(&dir.join("ca.key"), &key)?;
            write_atomic(&dir.join("ca.pem"), cert.pem().as_bytes())?;
            new_authority = true;
            key
        }
    };
    let ca_pem = std::fs::read_to_string(dir.join("ca.pem")).map_err(|e| format!("reading ca.pem: {e}"))?;
    let issuer = Issuer::from_ca_cert_pem(&ca_pem, &ca_key).map_err(|e| format!("reading the authority: {e}"))?;
    let key = KeyPair::generate().map_err(|e| format!("making the key: {e}"))?;
    let mut p = CertificateParams::default();
    p.distinguished_name = name("localhost");
    p.subject_alt_names = vec![SanType::DnsName("localhost".try_into().map_err(|e| format!("{e}"))?), SanType::IpAddress([127, 0, 0, 1].into())];
    p.is_ca = IsCa::ExplicitNoCa;
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.use_authority_key_identifier_extension = true;
    p.not_before = now - Duration::days(1);
    p.not_after = now + Duration::days(LEAF_DAYS);
    p.serial_number = Some(serial());
    let cert = p.signed_by(&key, &issuer).map_err(|e| format!("signing the certificate: {e}"))?;
    // made beside the old files and moved over them only when complete
    write_key(&dir.join("server.key"), &key)?;
    write_atomic(&dir.join("server.pem"), cert.pem().as_bytes())?;
    Ok(if new_authority { Outcome::NewAuthority } else { Outcome::Renewed })
}

fn name(cn: &str) -> DistinguishedName {
    let mut d = DistinguishedName::new();
    d.push(DnType::CommonName, cn);
    d
}

/// 16 random bytes, positive: browsers refuse two certificates from one issuer with one serial.
fn serial() -> SerialNumber {
    let mut b = *uuid::Uuid::new_v4().as_bytes();
    b[0] &= 0x7f;
    b[0] |= 0x01;
    SerialNumber::from_slice(&b)
}

/// Whether server.pem was signed by the authority in ca.pem (a new authority with an old
/// certificate left over, after a crash between the two writes, is not good).
fn leaf_from_authority(dir: &Path) -> bool {
    let (Some(leaf), Some(ca)) = (cert_der(&dir.join("server.pem")), cert_der(&dir.join("ca.pem"))) else { return false };
    let (Ok((_, l)), Ok((_, c))) = (x509_parser::parse_x509_certificate(&leaf), x509_parser::parse_x509_certificate(&ca)) else { return false };
    l.issuer() == c.subject() && l.verify_signature(Some(c.public_key())).is_ok()
}

/// The first certificate in a PEM file, as DER.
pub fn cert_der(pem_file: &Path) -> Option<Vec<u8>> {
    let text = std::fs::read(pem_file).ok()?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&text).ok()?;
    Some(pem.contents)
}

/// When a PEM certificate runs out.
pub fn expiry(pem_file: &Path) -> Option<OffsetDateTime> {
    let der = cert_der(pem_file)?;
    let (_, c) = x509_parser::parse_x509_certificate(&der).ok()?;
    Some(c.validity().not_after.to_datetime())
}

/// The TLS configuration the server presents: the localhost certificate and its key.
pub fn server_config(dir: &Path) -> Result<Arc<rustls::ServerConfig>, String> {
    let leaf = cert_der(&dir.join("server.pem")).ok_or("no certificate")?;
    let key = read_key(&dir.join("server.key"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(leaf)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())))
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(config))
}

/// Whether the localhost certificate in `dir` is valid for `host` at `now` when the authority in
/// `dir` is the only trusted root (what a browser checks once the authority is trusted: chain,
/// dates, server use, the name and the authority's name constraints).
#[cfg(test)]
pub fn verify(dir: &Path, host: &str, now: OffsetDateTime) -> bool {
    let (Some(leaf), Some(ca)) = (cert_der(&dir.join("server.pem")), cert_der(&dir.join("ca.pem"))) else { return false };
    verify_chain(&leaf, &ca, host, now)
}

#[cfg(test)]
pub fn verify_chain(leaf: &[u8], ca: &[u8], host: &str, now: OffsetDateTime) -> bool {
    use rustls::client::danger::ServerCertVerifier;
    use rustls::pki_types::{ServerName, UnixTime};
    let mut roots = rustls::RootCertStore::empty();
    if roots.add(CertificateDer::from(ca.to_vec())).is_err() {
        return false;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let Ok(verifier) = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider).build() else { return false };
    let Ok(name) = ServerName::try_from(host.to_string()) else { return false };
    let t = UnixTime::since_unix_epoch(std::time::Duration::from_secs(now.unix_timestamp().max(0) as u64));
    verifier.verify_server_cert(&CertificateDer::from(leaf.to_vec()), &[], &name, &[], t).is_ok()
}

// ---- keys, kept for this user only ----

fn read_key(path: &Path) -> Result<KeyPair, String> {
    let raw = std::fs::read(path).map_err(|e| format!("reading {}: {e}", file_name(path)))?;
    let der = unprotect(&raw)?;
    let pem = String::from_utf8(der).map_err(|_| format!("{} is damaged", file_name(path)))?;
    KeyPair::from_pem(&pem).map_err(|e| format!("{}: {e}", file_name(path)))
}

fn write_key(path: &Path, key: &KeyPair) -> Result<(), String> {
    write_atomic(path, &protect(key.serialize_pem().as_bytes())?)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// Writes beside the file and renames over it; readable by this user only (Unix).
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("new");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp).map_err(|e| format!("writing {}: {e}", file_name(path)))?;
        std::io::Write::write_all(&mut f, data).map_err(|e| format!("writing {}: {e}", file_name(path)))?;
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("writing {}: {e}", file_name(path)))
}

fn make_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}

/// DPAPI for this Windows user (CryptProtectData): only this account on this computer can read it.
#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    crate::citesetup::win::dpapi(data, true)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    crate::citesetup::win::dpapi(data, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lunta-tls-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn made_and_renewed_without_trusting_again() {
        let root = tmp();
        let dir = root.join("tls");
        let now = OffsetDateTime::now_utc();
        assert_eq!(ensure(&dir, now), Outcome::NewAuthority);
        assert_eq!(ensure(&dir, now), Outcome::Current);
        assert!(server_config(&dir).is_ok());
        // valid for localhost and 127.0.0.1 with the authority trusted (name constraints and all)
        assert!(verify(&dir, "localhost", now));
        assert!(verify(&dir, "127.0.0.1", now));
        assert!(!verify(&dir, "example.com", now));
        let left = expiry(&dir.join("server.pem")).unwrap() - now;
        assert!(left > Duration::days(800), "{left}");
        let ca = std::fs::read(dir.join("ca.pem")).unwrap();
        let first = std::fs::read(dir.join("server.pem")).unwrap();
        // two years on (its last month): a new localhost certificate from the same, still trusted, authority
        let later = now + Duration::days(800);
        assert_eq!(ensure(&dir, later), Outcome::Renewed);
        assert_ne!(std::fs::read(dir.join("server.pem")).unwrap(), first);
        assert_eq!(std::fs::read(dir.join("ca.pem")).unwrap(), ca, "the same authority");
        assert!(verify(&dir, "localhost", later));
        assert!(!verify(&dir, "localhost", later + Duration::days(LEAF_DAYS + 1)), "runs out");
        // keys readable by this user only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("ca.key")).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(dir.join("server.key")).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        // an authority whose key is gone: kept while the certificate is good, replaced when it runs out
        std::fs::remove_file(dir.join("ca.key")).unwrap();
        assert_eq!(ensure(&dir, later), Outcome::Current);
        let even_later = later + Duration::days(800);
        assert_eq!(ensure(&dir, even_later), Outcome::NewAuthority);
        assert_ne!(std::fs::read(dir.join("ca.pem")).unwrap(), ca);
        let leaf = cert_der(&dir.join("server.pem")).unwrap();
        let old_ca = x509_parser::pem::parse_x509_pem(&ca).unwrap().1.contents;
        assert!(!verify_chain(&leaf, &old_ca, "localhost", even_later), "the old authority no longer vouches for it");
        assert!(verify(&dir, "localhost", even_later));
        // the authority itself runs out within the next certificate's life: a new one
        let near_end = even_later + Duration::days(CA_DAYS - LEAF_DAYS);
        assert_eq!(ensure(&dir, near_end), Outcome::NewAuthority);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_authority_vouches_only_for_this_computer() {
        let root = tmp();
        let dir = root.join("tls");
        let now = OffsetDateTime::now_utc();
        assert_eq!(ensure(&dir, now), Outcome::NewAuthority);
        let ca_der = cert_der(&dir.join("ca.pem")).unwrap();
        let (_, ca) = x509_parser::parse_x509_certificate(&ca_der).unwrap();
        let nc = ca.name_constraints().unwrap().expect("name constraints");
        assert!(nc.critical);
        assert!(ca.basic_constraints().unwrap().unwrap().value.ca);
        // someone with the authority's key signs a certificate for another site: no browser takes it
        let ca_key = read_key(&dir.join("ca.key")).unwrap();
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_der.clone()), &ca_key).unwrap();
        for host in ["example.com", "localhost.example.com", "10.0.0.1"] {
            let key = KeyPair::generate().unwrap();
            let mut p = CertificateParams::new(vec![host.to_string()]).unwrap();
            p.not_before = now - Duration::days(1);
            p.not_after = now + Duration::days(30);
            p.use_authority_key_identifier_extension = true;
            let evil = p.signed_by(&key, &issuer).unwrap();
            assert!(!verify_chain(evil.der(), &ca_der, host, now), "{host} must be refused");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_damaged_key_makes_a_new_authority() {
        let root = tmp();
        let dir = root.join("tls");
        let now = OffsetDateTime::now_utc();
        ensure(&dir, now);
        std::fs::write(dir.join("ca.key"), b"nonsense").unwrap();
        std::fs::write(dir.join("server.key"), b"nonsense").unwrap();
        assert_eq!(ensure(&dir, now), Outcome::NewAuthority);
        assert!(verify(&dir, "localhost", now));
        let _ = std::fs::remove_dir_all(&root);
    }
}
