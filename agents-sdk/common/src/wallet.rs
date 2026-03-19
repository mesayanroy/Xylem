//! Stellar wallet: Strkey decode, ed25519 keypair management, transaction signing.
//!
//! Stellar uses **Ed25519** keys encoded with **Strkey** (a base32 encoding with
//! a version byte + CRC-16/CCITT-XModem checksum).
//!
//! Secret seed  → starts with `S` (version byte 0x90)
//! Public key   → starts with `G` (version byte 0x30)
//!
//! Signing algorithm:
//! ```text
//! tx_hash = SHA-256( SHA-256(network_passphrase) ‖ [0,0,0,2] ‖ tx_xdr_bytes )
//! signature = ed25519_sign(signing_key, tx_hash)
//! ```

use crate::config::STROOPS_PER_XLM;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use crc::{Crc, CRC_16_IBM_3740};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

const CRC16: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_3740);

/// Version bytes for Stellar Strkey encoding.
const STRKEY_SEED_VERSION: u8 = 0x90;   // private key seed  → "S…"
const STRKEY_PUBKEY_VERSION: u8 = 0x30; // Ed25519 public key → "G…"

/// Standard base-32 alphabet (RFC 4648, no padding).
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("Invalid Strkey length: expected 56 chars, got {0}")]
    InvalidLength(usize),
    #[error("Unsupported version byte: 0x{0:02X}")]
    UnsupportedVersion(u8),
    #[error("Checksum mismatch (Strkey is corrupted or wrong network)")]
    BadChecksum,
    #[error("Base32 decode failed: {0}")]
    Base32Error(String),
    #[error("Cryptographic error: {0}")]
    CryptoError(String),
}

// ── Strkey helpers ────────────────────────────────────────────────────────────

/// Decode a Stellar Strkey string into raw bytes.
///
/// Returns `(version_byte, payload_bytes)`.
fn strkey_decode(input: &str) -> std::result::Result<(u8, Vec<u8>), WalletError> {
    let input = input.trim();
    if input.len() != 56 {
        return Err(WalletError::InvalidLength(input.len()));
    }

    // ── Base32 → bytes (56 chars × 5 bits = 280 bits = 35 bytes)
    let raw = base32_decode(input).map_err(WalletError::Base32Error)?;
    // raw = [version_byte, payload (32 bytes), crc_hi, crc_lo]
    assert_eq!(raw.len(), 35, "Strkey raw bytes must be 35");

    let version = raw[0];
    let payload = raw[1..33].to_vec();
    let stored_crc = u16::from_le_bytes([raw[33], raw[34]]);
    let computed_crc = CRC16.checksum(&raw[..33]);

    if stored_crc != computed_crc {
        return Err(WalletError::BadChecksum);
    }

    Ok((version, payload))
}

/// Encode raw bytes into Stellar Strkey format.
pub fn strkey_encode(version: u8, payload: &[u8; 32]) -> String {
    let mut data = Vec::with_capacity(35);
    data.push(version);
    data.extend_from_slice(payload);
    let crc = CRC16.checksum(&data);
    data.push(crc as u8);         // little-endian lo byte
    data.push((crc >> 8) as u8);  // little-endian hi byte
    base32_encode(&data)
}

fn base32_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    let mut bits: u64 = 0;
    let mut bit_count: u32 = 0;
    let mut output = Vec::with_capacity(35);

    for c in input.bytes() {
        let val = BASE32_ALPHABET
            .iter()
            .position(|&b| b == c)
            .ok_or_else(|| format!("Invalid base32 char: {c}"))?;
        bits = (bits << 5) | val as u64;
        bit_count += 5;
        if bit_count >= 8 {
            bit_count -= 8;
            output.push((bits >> bit_count) as u8 & 0xFF);
        }
    }
    Ok(output)
}

fn base32_encode(input: &[u8]) -> String {
    let mut result = Vec::with_capacity(56);
    let mut bits: u32 = 0;
    let mut bit_count: u32 = 0;

    for &byte in input {
        bits = (bits << 8) | byte as u32;
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            result.push(BASE32_ALPHABET[(bits >> bit_count) as usize & 0x1F]);
        }
    }
    if bit_count > 0 {
        result.push(BASE32_ALPHABET[(bits << (5 - bit_count)) as usize & 0x1F]);
    }

    String::from_utf8(result).expect("base32 alphabet is ASCII")
}

// ── Keypair ───────────────────────────────────────────────────────────────────

/// An Ed25519 Stellar keypair derived from a secret seed.
#[derive(Clone)]
pub struct Keypair {
    signing_key:  SigningKey,
    verifying_key: VerifyingKey,
    /// Base58-like "G…" address used in Horizon API calls.
    pub public_key: String,
}

impl Keypair {
    /// Load a keypair from a Stellar secret key string `S…`.
    pub fn from_secret(secret: &str) -> std::result::Result<Self, WalletError> {
        let (version, seed_bytes) = strkey_decode(secret)?;
        if version != STRKEY_SEED_VERSION {
            return Err(WalletError::UnsupportedVersion(version));
        }

        let seed: [u8; 32] = seed_bytes
            .try_into()
            .map_err(|_| WalletError::CryptoError("seed length != 32".into()))?;

        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let public_key = strkey_encode(STRKEY_PUBKEY_VERSION, verifying_key.as_bytes());

        Ok(Self { signing_key, verifying_key, public_key })
    }

    /// Stellar transaction signing.
    ///
    /// `tx_hash = SHA-256( network_id_hash ‖ [0,0,0,2] ‖ tx_xdr )`
    ///
    /// Returns `(hint_4_bytes, signature_64_bytes)` to embed in `DecoratedSignature`.
    pub fn sign_transaction(
        &self,
        network_passphrase: &str,
        tx_xdr: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let network_id = Sha256::digest(network_passphrase.as_bytes());

        let mut payload = Vec::with_capacity(32 + 4 + tx_xdr.len());
        payload.extend_from_slice(&network_id);
        payload.extend_from_slice(&[0u8, 0, 0, 2]); // ENVELOPE_TYPE_TX
        payload.extend_from_slice(tx_xdr);

        let tx_hash = Sha256::digest(&payload);
        let signature: Signature = self.signing_key.sign(&tx_hash);

        let hint = self.verifying_key.as_bytes()[28..32].to_vec(); // last 4 bytes
        (hint, signature.to_bytes().to_vec())
    }

    /// Raw SHA-256 hash of any arbitrary message (useful for off-chain signing).
    pub fn sign_hash(&self, message: &[u8]) -> Vec<u8> {
        let hash = Sha256::digest(message);
        let sig: Signature = self.signing_key.sign(&hash);
        sig.to_bytes().to_vec()
    }

    /// Base64-encode the 64-byte signature (for embedding in JSON / HTTP headers).
    pub fn sign_transaction_b64(&self, network_passphrase: &str, tx_xdr: &[u8]) -> String {
        let (_hint, sig) = self.sign_transaction(network_passphrase, tx_xdr);
        B64.encode(&sig)
    }

    /// The raw 32-byte Ed25519 public key (for contract invocations).
    pub fn raw_public_key(&self) -> [u8; 32] {
        *self.verifying_key.as_bytes()
    }
}

impl std::fmt::Debug for Keypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keypair")
            .field("public_key", &self.public_key)
            .finish_non_exhaustive()
    }
}

// ── Public-key decoding for stellar_tx ───────────────────────────────────────

/// Decode a Stellar `G…` public key into its raw 32-byte Ed25519 key.
///
/// This is used internally by [`crate::stellar_tx`] when building XDR.
pub fn decode_raw_public_key(gaddress: &str) -> std::result::Result<[u8; 32], WalletError> {
    let (version, payload) = strkey_decode(gaddress)?;
    if version != STRKEY_PUBKEY_VERSION {
        return Err(WalletError::UnsupportedVersion(version));
    }
    payload
        .try_into()
        .map_err(|_| WalletError::CryptoError("public key length != 32".into()))
}

// ── XLM conversion helpers ────────────────────────────────────────────────────

/// Convert XLM to stroops (1 XLM = 10_000_000 stroops).
#[inline(always)]
pub fn xlm_to_stroops(xlm: f64) -> i64 {
    (xlm * STROOPS_PER_XLM as f64).round() as i64
}

/// Convert stroops to XLM.
#[inline(always)]
pub fn stroops_to_xlm(stroops: i64) -> f64 {
    stroops as f64 / STROOPS_PER_XLM as f64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Well-known Stellar testnet account (SDF friendbot).
    const KNOWN_SECRET: &str = "SCZANGBA5RLBRQ46SL6GFBFXQ3QJYR57G5YHC7VKPLLK2NNZRHIBSG";

    #[test]
    fn roundtrip_strkey() {
        let (ver, payload) = strkey_decode(KNOWN_SECRET).unwrap();
        assert_eq!(ver, STRKEY_SEED_VERSION);
        let reencoded = strkey_encode(ver, payload.as_slice().try_into().unwrap());
        assert_eq!(reencoded, KNOWN_SECRET);
    }

    #[test]
    fn keypair_derives_gaddress() {
        let kp = Keypair::from_secret(KNOWN_SECRET).unwrap();
        assert!(kp.public_key.starts_with('G'));
        assert_eq!(kp.public_key.len(), 56);
    }

    #[test]
    fn stroop_conversion() {
        assert_eq!(xlm_to_stroops(1.0), 10_000_000);
        assert!((stroops_to_xlm(10_000_000) - 1.0).abs() < 1e-9);
    }
}
