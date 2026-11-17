// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Authenticate Source bookkeeping only; no Native/App/Core authority is signed.
use hmac::Hmac;
use hmac::Mac;
use sha2::Sha256;
pub(super) struct Auth {
    key: [u8; 32],
    pub epoch: String,
}
impl Auth {
    pub(super) fn new() -> Self {
        let mut key = [0; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self {
            key,
            epoch: uuid::Uuid::new_v4().simple().to_string(),
        }
    }
    pub(super) fn sign(
        &self,
        domain: &str,
        value: &impl serde::Serialize,
    ) -> std::io::Result<String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).map_err(std::io::Error::other)?;
        mac.update(domain.as_bytes());
        mac.update(&[0]);
        mac.update(&serde_json::to_vec(value).map_err(std::io::Error::other)?);
        Ok(mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    pub(super) fn verify(
        &self,
        domain: &str,
        value: &impl serde::Serialize,
        proof: &str,
    ) -> std::io::Result<bool> {
        if proof.len() != 64
            || !proof
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Ok(false);
        }
        let parsed = (0..32)
            .map(|i| u8::from_str_radix(&proof[i * 2..i * 2 + 2], 16))
            .collect::<Result<Vec<_>, _>>();
        let Ok(parsed) = parsed else { return Ok(false) };
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).map_err(std::io::Error::other)?;
        mac.update(domain.as_bytes());
        mac.update(&[0]);
        mac.update(&serde_json::to_vec(value).map_err(std::io::Error::other)?);
        Ok(mac.verify_slice(&parsed).is_ok())
    }
}

#[cfg(test)]
#[path = "retained_callback_auth_tests.rs"]
mod tests;
