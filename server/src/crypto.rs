//! Sealing stored credentials (AES-256-GCM) and signing JWTs (RS256), both on
//! ring, which rustls already brings in.
//!
//! The sealing key comes from `ANALYTICS_SECRET` (a Realm stable secret, see
//! realm.tsx) or, without it, from a random `secret` file in the data dir, so
//! `pnpm dev:server` needs no setup. Losing the key only loses the stored
//! Search Console keys; users paste them again.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use std::path::Path;

pub struct Sealer(LessSafeKey);

impl Sealer {
    /// Any secret string; it is hashed to the 256-bit key.
    pub fn new(secret: &str) -> Self {
        let key = digest(&SHA256, secret.as_bytes());
        Self(LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, key.as_ref()).expect("32-byte key"),
        ))
    }

    /// The env secret, else the data dir's `secret` file, created on first run.
    pub fn from_env_or_file(dir: &Path) -> std::io::Result<Self> {
        if let Some(secret) = std::env::var("ANALYTICS_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
        {
            return Ok(Self::new(&secret));
        }
        let path = dir.join("secret");
        if !path.exists() {
            let bytes = crate::visitor::random_bytes::<32>();
            std::fs::write(&path, STANDARD.encode(bytes))?;
        }
        Ok(Self::new(std::fs::read_to_string(path)?.trim()))
    }

    /// `base64(nonce || ciphertext || tag)`.
    pub fn seal(&self, plain: &str) -> String {
        let nonce = crate::visitor::random_bytes::<NONCE_LEN>();
        let mut buf = plain.as_bytes().to_vec();
        self.0
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut buf)
            .expect("seal");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&buf);
        STANDARD.encode(out)
    }

    /// `None` when the data was sealed with another key or is damaged.
    pub fn open(&self, sealed: &str) -> Option<String> {
        let bytes = STANDARD.decode(sealed).ok()?;
        if bytes.len() < NONCE_LEN {
            return None;
        }
        let (nonce, rest) = bytes.split_at(NONCE_LEN);
        let mut buf = rest.to_vec();
        let plain = self
            .0
            .open_in_place(
                Nonce::try_assume_unique_for_key(nonce).ok()?,
                Aad::empty(),
                &mut buf,
            )
            .ok()?;
        String::from_utf8(plain.to_vec()).ok()
    }
}

/// A signed RS256 JWT for these claims. `pem` is a PKCS#8 private key, as in
/// a Google service account file.
pub fn jwt(pem: &str, key_id: Option<&str>, claims: &serde_json::Value) -> Result<String, String> {
    let der = pem_body(pem).ok_or("private_key is not a PEM PKCS#8 key")?;
    let key = RsaKeyPair::from_pkcs8(&der).map_err(|e| format!("private_key: {e}"))?;
    let mut header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
    if let Some(kid) = key_id {
        header["kid"] = kid.into();
    }
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string()),
    );
    let mut sig = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut sig,
    )
    .map_err(|_| "signing failed".to_string())?;
    Ok(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig)))
}

fn pem_body(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != "-----BEGIN PRIVATE KEY-----")
        .skip(1)
        .take_while(|l| *l != "-----END PRIVATE KEY-----")
        .collect();
    STANDARD.decode(body).ok().filter(|b| !b.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    const TEST_KEY: &str = include_str!("testdata/test-key.pem");
    const TEST_PUB: &[u8] = include_bytes!("testdata/test-key.pub.der");

    #[test]
    fn sealed_values_open_only_with_the_same_key() {
        let a = Sealer::new("one");
        let sealed = a.seal("hello");
        assert_ne!(sealed, a.seal("hello"), "fresh nonce every time");
        assert_eq!(a.open(&sealed).as_deref(), Some("hello"));
        assert_eq!(Sealer::new("two").open(&sealed), None);
        assert_eq!(a.open("not base64!"), None);
        assert_eq!(a.open(""), None);
    }

    #[test]
    fn the_secret_file_is_created_once() {
        let dir = std::env::temp_dir().join(format!("analytics-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sealed = Sealer::from_env_or_file(&dir).unwrap().seal("x");
        let again = Sealer::from_env_or_file(&dir).unwrap();
        assert_eq!(again.open(&sealed).as_deref(), Some("x"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn jwts_verify_against_the_public_key() {
        let claims = serde_json::json!({ "iss": "a@b", "exp": 1 });
        let token = jwt(TEST_KEY, Some("kid-1"), &claims).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["kid"], "kid-1");
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, TEST_PUB)
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .expect("signature verifies");
        assert!(jwt("not a key", None, &claims).is_err());
    }
}
