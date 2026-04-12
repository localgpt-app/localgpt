//! Auto-generated self-signed TLS certificates for HTTPS server.
//!
//! On first run, generates a CA + server certificate pair and stores them locally.
//! Auto-regenerates when certificates expire or SANs change.
//!
//! Follows the pattern from Moltis (`crates/tls/`): self-signed CA + server cert,
//! stored at `~/.config/localgpt/certs/`, with SAN detection for hostname + LAN IPs.

#[cfg(feature = "tls")]
pub mod certs {
    use anyhow::{Result, anyhow};
    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
        KeyUsagePurpose, SanType,
    };
    use std::fs;
    use std::net::IpAddr;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tracing::{debug, info};

    /// Metadata about generated certificates.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct CertMeta {
        pub sans: Vec<String>,
        pub ca_expires_at: i64,
        pub server_expires_at: i64,
        pub generated_at: i64,
    }

    /// All paths for certificate storage.
    pub struct CertPaths {
        pub dir: PathBuf,
        pub ca_cert: PathBuf,
        pub ca_key: PathBuf,
        pub server_cert: PathBuf,
        pub server_key: PathBuf,
        pub meta: PathBuf,
    }

    impl CertPaths {
        pub fn new(dir: &Path) -> Self {
            Self {
                dir: dir.to_path_buf(),
                ca_cert: dir.join("ca.pem"),
                ca_key: dir.join("ca-key.pem"),
                server_cert: dir.join("server.pem"),
                server_key: dir.join("server-key.pem"),
                meta: dir.join("meta.json"),
            }
        }
    }

    /// Check if existing certs need regeneration.
    pub fn needs_regeneration(paths: &CertPaths, renew_threshold_days: u32) -> bool {
        // No meta file = first run
        if !paths.meta.exists() {
            return true;
        }

        let meta = match load_meta(paths) {
            Ok(m) => m,
            Err(_) => return true,
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let threshold_secs = renew_threshold_days as i64 * 86400;

        // Check expiry
        if meta.server_expires_at - now < threshold_secs {
            info!("Server certificate expires soon, regenerating");
            return true;
        }

        // Check SANs changed
        let current_sans = detect_sans();
        if current_sans != meta.sans {
            info!("SANs changed, regenerating certificates");
            return true;
        }

        false
    }

    /// Generate CA + server certificate pair.
    pub fn generate_certs(paths: &CertPaths) -> Result<()> {
        fs::create_dir_all(&paths.dir)?;

        let sans = detect_sans();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Generate CA (10 years)
        let ca_key = KeyPair::generate()?;
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let mut ca_dn = DistinguishedName::new();
        ca_dn.push(DnType::CommonName, "LocalGPT CA");
        ca_dn.push(DnType::OrganizationName, "LocalGPT");
        ca_params.distinguished_name = ca_dn;
        // 10 years validity
        ca_params.not_after = rcgen::date_time_ymd(2036, 1, 1);

        let ca_cert = ca_params.self_signed(&ca_key)?;

        // Generate server cert (1 year), signed by CA
        let server_key = KeyPair::generate()?;
        let mut server_params = CertificateParams::default();
        server_params.is_ca = IsCa::NoCa;
        let mut server_dn = DistinguishedName::new();
        server_dn.push(DnType::CommonName, "LocalGPT Server");
        server_params.distinguished_name = server_dn;
        // 1 year validity
        server_params.not_after = rcgen::date_time_ymd(2027, 4, 11);

        // Add SANs
        let san_types: Vec<SanType> = sans
            .iter()
            .map(|s| {
                if let Ok(ip) = s.parse::<IpAddr>() {
                    SanType::IpAddress(ip)
                } else {
                    SanType::DnsName(
                        s.clone()
                            .try_into()
                            .unwrap_or_else(|_| "localhost".to_string().try_into().unwrap()),
                    )
                }
            })
            .collect();
        server_params.subject_alt_names = san_types;

        let server_cert = server_params.signed_by(&server_key, &ca_cert, &ca_key)?;

        // Write files
        fs::write(&paths.ca_cert, ca_cert.pem())?;
        fs::write(&paths.ca_key, ca_key.serialize_pem())?;
        fs::write(&paths.server_cert, server_cert.pem())?;
        fs::write(&paths.server_key, server_key.serialize_pem())?;

        // Restrict key file permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&paths.ca_key, fs::Permissions::from_mode(0o600))?;
            fs::set_permissions(&paths.server_key, fs::Permissions::from_mode(0o600))?;
        }

        // Write metadata
        let ca_expires_at = now + 10 * 365 * 86400; // ~10 years
        let server_expires_at = now + 365 * 86400; // ~1 year
        let meta = CertMeta {
            sans,
            ca_expires_at,
            server_expires_at,
            generated_at: now,
        };
        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(&paths.meta, meta_json)?;

        info!("Generated TLS certificates at {}", paths.dir.display());
        debug!("SANs: {:?}", meta.sans);

        Ok(())
    }

    /// Load or generate certificates, returning paths to cert + key.
    pub fn ensure_certs(cert_dir: &Path, renew_threshold_days: u32) -> Result<CertPaths> {
        let paths = CertPaths::new(cert_dir);

        if needs_regeneration(&paths, renew_threshold_days) {
            generate_certs(&paths)?;
        } else {
            debug!(
                "Using existing TLS certificates from {}",
                cert_dir.display()
            );
        }

        // Verify files exist
        if !paths.server_cert.exists() || !paths.server_key.exists() {
            return Err(anyhow!(
                "TLS certificate files missing at {}",
                cert_dir.display()
            ));
        }

        Ok(paths)
    }

    fn load_meta(paths: &CertPaths) -> Result<CertMeta> {
        let content = fs::read_to_string(&paths.meta)?;
        let meta: CertMeta = serde_json::from_str(&content)?;
        Ok(meta)
    }

    /// Detect Subject Alternative Names (hostname + local IPs).
    fn detect_sans() -> Vec<String> {
        let mut sans = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ];

        // Add hostname
        if let Ok(hostname) = hostname::get() {
            let hostname_str = hostname.to_string_lossy().to_string();
            if !sans.contains(&hostname_str) {
                sans.push(hostname_str);
            }
        }

        // Add LAN IPs (best effort)
        if let Ok(addrs) = local_ip_address::list_afinet_netifas() {
            for (_, ip) in addrs {
                let ip_str = ip.to_string();
                if !ip.is_loopback() && !sans.contains(&ip_str) {
                    sans.push(ip_str);
                }
            }
        }

        sans.sort();
        sans
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        #[test]
        fn test_generate_and_load_certs() {
            let dir = TempDir::new().unwrap();
            let paths = CertPaths::new(dir.path());

            generate_certs(&paths).unwrap();

            assert!(paths.ca_cert.exists());
            assert!(paths.ca_key.exists());
            assert!(paths.server_cert.exists());
            assert!(paths.server_key.exists());
            assert!(paths.meta.exists());

            let meta = load_meta(&paths).unwrap();
            assert!(!meta.sans.is_empty());
            assert!(meta.sans.contains(&"localhost".to_string()));
        }

        #[test]
        fn test_needs_regeneration_first_run() {
            let dir = TempDir::new().unwrap();
            let paths = CertPaths::new(dir.path());
            assert!(needs_regeneration(&paths, 30));
        }

        #[test]
        fn test_no_regeneration_when_fresh() {
            let dir = TempDir::new().unwrap();
            let paths = CertPaths::new(dir.path());

            generate_certs(&paths).unwrap();

            assert!(!needs_regeneration(&paths, 30));
        }

        #[test]
        fn test_ensure_certs_idempotent() {
            let dir = TempDir::new().unwrap();

            let paths = ensure_certs(dir.path(), 30).unwrap();
            assert!(paths.server_cert.exists());

            // Second call should reuse existing
            let paths2 = ensure_certs(dir.path(), 30).unwrap();
            assert!(paths2.server_cert.exists());
        }

        #[test]
        fn test_detect_sans() {
            let sans = detect_sans();
            assert!(sans.contains(&"localhost".to_string()));
            assert!(sans.contains(&"127.0.0.1".to_string()));
        }
    }
}
