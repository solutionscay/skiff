//! Server bundles: `skiff-server-<version>-<os>-<arch>.tar.gz`, built by the
//! release job and signed with minisign. The app carries the public key and
//! checks a bundle before it uses it. A bundle that fails the check is never
//! unpacked, uploaded or run.
//!
//! The signature's trusted comment names the file (`file:<name>`), so a
//! valid signature of another bundle, such as an older version, does not
//! pass for this one.

use serde::{Deserialize, Serialize};

/// The release key's public half, set by the release job at build time
/// (`SKIFF_SERVER_PUBLIC_KEY`, the base64 line of a minisign public key).
/// `None` in a build without it: no bundle passes.
pub const PUBLIC_KEY: Option<&str> = option_env!("SKIFF_SERVER_PUBLIC_KEY");

/// `manifest.json` in a bundle.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub protocol: u32,
    pub os: String,
    pub arch: String,
    pub target: String,
    pub commit: String,
    /// SHA-256 of each file in the bundle, by its path in the bundle.
    pub files: std::collections::BTreeMap<String, String>,
}

/// The bundle name for a version, OS and arch, as the release job writes it.
pub fn bundle_name(version: &str, os: &str, arch: &str) -> String {
    format!("skiff-server-{version}-{os}-{arch}.tar.gz")
}

/// Checks `bytes` against its `.minisig` with this build's key.
pub fn verify(name: &str, bytes: &[u8], minisig: &str) -> Result<(), String> {
    let Some(key) = PUBLIC_KEY.filter(|k| !k.trim().is_empty()) else {
        return Err("This build of Skiff has no key to check server bundles. Skiff did not use the bundle.".into());
    };
    verify_with(key, name, bytes, minisig)
}

/// Checks `bytes`, named `name`, against `minisig` and the minisign public
/// key `key` (its base64 line, or the whole `.pub` file).
pub fn verify_with(key: &str, name: &str, bytes: &[u8], minisig: &str) -> Result<(), String> {
    let refused = |why: &str| Err(format!("{name} is not signed by Skiff ({why}). Skiff did not use it."));
    let line = key.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("untrusted comment:")).unwrap_or_default();
    let Ok(public) = minisign_verify::PublicKey::from_base64(line) else {
        return refused("the built-in key is not a minisign key");
    };
    let Ok(sig) = minisign_verify::Signature::decode(minisig) else {
        return refused("the signature file is not a minisign signature");
    };
    if public.verify(bytes, &sig, false).is_err() {
        return refused("the signature does not match");
    }
    if sig.trusted_comment() != format!("file:{name}") {
        return refused("the signature is for another file");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key, made for this test with minisign 0.12; its secret
    // half was deleted. It signs only the message below.
    const KEY: &str = "untrusted comment: minisign public key 3E49236E79CBBE7C\nRWR8vst5biNJPtw43YgsPkt7jCLT3q7bDPRl24Lho80Og6qV9MYWraRt\n";
    const NAME: &str = "skiff-server-0.0.0-linux-x86_64.tar.gz";
    const BYTES: &[u8] = b"skiff-server test bundle\n";
    const SIG: &str = "untrusted comment: signature from minisign secret key\n\
RUR8vst5biNJPtPkMbm6s3C8IFtGIeslMRudbLZF8PH94skH8tngLjiBy9FWDKlD4Es4UHHJQFDRYIU56lIT8tj522enoIrVLAU=\n\
trusted comment: file:skiff-server-0.0.0-linux-x86_64.tar.gz\n\
mNE7KC4NXZ3+jDHRVzwnNFPVzEL8fHMG754TiG5vCdXtT98GpoPcMuF221TKeqvTRx5ahcm/NQmYrNFUwsg5CQ==\n";

    #[test]
    fn a_signed_bundle_passes_and_anything_else_is_refused() {
        assert_eq!(verify_with(KEY, NAME, BYTES, SIG), Ok(()));
        // The base64 line alone works too.
        assert_eq!(verify_with(KEY.lines().nth(1).unwrap(), NAME, BYTES, SIG), Ok(()));
        let tampered = b"skiff-server test bundlE\n";
        assert!(verify_with(KEY, NAME, tampered, SIG).unwrap_err().contains("does not match"));
        // A valid signature of another file does not pass for this one.
        let other = bundle_name("0.0.1", "linux", "x86_64");
        assert!(verify_with(KEY, &other, BYTES, SIG).unwrap_err().contains("another file"));
        let wrong_key = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        assert!(verify_with(wrong_key, NAME, BYTES, SIG).is_err());
        assert!(verify_with(KEY, NAME, BYTES, "not a signature").is_err());
        assert!(verify_with("", NAME, BYTES, SIG).is_err());
    }

    #[test]
    fn a_build_without_the_release_key_uses_no_bundle() {
        if PUBLIC_KEY.is_none() {
            assert!(verify(NAME, BYTES, SIG).unwrap_err().contains("no key"));
        }
    }

    #[test]
    fn the_manifest_reads_as_the_release_job_writes_it() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"skiff-server","version":"1.6.6","protocol":27,"os":"linux","arch":"x86_64","target":"x86_64-unknown-linux-musl","commit":"e8368d7","files":{"skiffd":"ab","skiff-host":"cd","skiff":"ef","systemd/skiffd.service":"01","systemd/skiff-host.service":"02"}}"#,
        )
        .unwrap();
        assert_eq!(bundle_name(&m.version, &m.os, &m.arch), "skiff-server-1.6.6-linux-x86_64.tar.gz");
        assert_eq!(m.files.len(), 5);
    }
}
