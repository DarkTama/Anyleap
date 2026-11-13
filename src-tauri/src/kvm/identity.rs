//! Self-signed identity, certificate-fingerprint trust store and the
//! permissive TLS verifiers (authorisation happens on the fingerprint).
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error as TlsError, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const ALPN: &[u8] = b"anyleap-kvm/1";

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

/// SHA-256 of the certificate DER, lowercase hex.
pub fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert).iter().map(|b| format!("{b:02x}")).collect()
}

pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: Vec<u8>,
}

impl Identity {
    /// Load `kvm_identity.der` / `kvm_identity.key` from `dir`, generating them on first use.
    pub fn load_or_create(dir: &Path) -> Result<Self, String> {
        let (cp, kp) = (dir.join("kvm_identity.der"), dir.join("kvm_identity.key"));
        if let (Ok(cert), Ok(key)) = (std::fs::read(&cp), std::fs::read(&kp)) {
            return Ok(Self { cert: cert.into(), key });
        }
        let ck = rcgen::generate_simple_self_signed(vec!["anyleap-kvm".to_string()])
            .map_err(|e| format!("Identity generation failed: {e}"))?;
        let (cert, key) = (ck.cert.der().to_vec(), ck.key_pair.serialize_der());
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&cp, &cert).map_err(|e| e.to_string())?;
        std::fs::write(&kp, &key).map_err(|e| e.to_string())?;
        Ok(Self { cert: cert.into(), key })
    }

    pub fn key_der(&self) -> PrivatePkcs8KeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.key.clone())
    }

    /// Stable 16-hex-char identifier derived from the local certificate fingerprint.
    pub fn computer_id(&self) -> String {
        fingerprint(&self.cert).chars().take(16).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustedPeer {
    pub computer_id: String,
    pub name: String,
    pub fingerprint: String,
    /// IP the peer was paired at (client-side lookup before the handshake).
    #[serde(default)]
    pub address: String,
}

pub struct TrustStore {
    path: Option<PathBuf>,
    peers: Mutex<Vec<TrustedPeer>>,
}

impl TrustStore {
    /// `dir = None` keeps the store in memory only (tests).
    pub fn load(dir: Option<&Path>) -> Self {
        let path = dir.map(|d| d.join("kvm_trusted_peers.json"));
        let peers = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { path, peers: Mutex::new(peers) }
    }

    fn save(&self, peers: &[TrustedPeer]) {
        if let (Some(p), Ok(json)) = (&self.path, serde_json::to_vec_pretty(peers)) {
            let _ = std::fs::write(p, json);
        }
    }

    pub fn contains(&self, fp: &str) -> bool {
        self.peers.lock().unwrap().iter().any(|p| p.fingerprint == fp)
    }

    pub fn by_address(&self, addr: &str) -> Option<TrustedPeer> {
        self.peers.lock().unwrap().iter().find(|p| p.address == addr).cloned()
    }

    pub fn by_fingerprint(&self, fp: &str) -> Option<TrustedPeer> {
        self.peers.lock().unwrap().iter().find(|p| p.fingerprint == fp).cloned()
    }

    /// Insert or replace (by `computer_id`).
    pub fn add(&self, peer: TrustedPeer) {
        let mut g = self.peers.lock().unwrap();
        g.retain(|p| p.computer_id != peer.computer_id);
        g.push(peer);
        self.save(&g);
    }

    pub fn remove(&self, computer_id: &str) {
        let mut g = self.peers.lock().unwrap();
        g.retain(|p| p.computer_id != computer_id);
        self.save(&g);
    }
}

/// Accepts any self-signed cert at the TLS layer; handshake signatures are
/// still verified, so the peer must hold the key of the cert it presents.
#[derive(Debug)]
pub struct AnyCert(Arc<CryptoProvider>);

impl AnyCert {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(provider()))
    }
}

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl ClientCertVerifier for AnyCert {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: UnixTime) -> Result<ClientCertVerified, TlsError> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Fingerprint of the peer's certificate on an established connection.
pub fn peer_fingerprint(conn: &quinn::Connection) -> Option<String> {
    let id = conn.peer_identity()?;
    let certs = id.downcast::<Vec<CertificateDer<'static>>>().ok()?;
    certs.first().map(|c| fingerprint(c))
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(std::time::Duration::from_secs(2)));
    t.max_idle_timeout(Some(std::time::Duration::from_secs(10).try_into().unwrap()));
    Arc::new(t)
}

pub fn server_config(id: &Identity) -> Result<quinn::ServerConfig, String> {
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(AnyCert::new())
        .with_single_cert(vec![id.cert.clone()], id.key_der().into())
        .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let q = quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(q));
    cfg.transport_config(transport_config());
    Ok(cfg)
}

pub fn client_config(id: &Identity) -> Result<quinn::ClientConfig, String> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(AnyCert::new())
        .with_client_auth_cert(vec![id.cert.clone()], id.key_der().into())
        .map_err(|e| e.to_string())?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let q = quinn::crypto::rustls::QuicClientConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut cfg = quinn::ClientConfig::new(Arc::new(q));
    cfg.transport_config(transport_config());
    Ok(cfg)
}

/// Constant-time equality (lengths are public).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computer_id_is_first_16_hex_chars_of_fingerprint() {
        let dir = std::env::temp_dir().join(format!("anyleap-id-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let id = Identity::load_or_create(&dir).unwrap();
        let fp = fingerprint(&id.cert);
        let cid = id.computer_id();
        assert_eq!(cid.len(), 16);
        assert_eq!(cid, &fp[..16]);
        assert!(cid.chars().all(|c| c.is_ascii_hexdigit()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
