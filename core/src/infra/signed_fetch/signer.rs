//! EIP-191 `personal_sign` over secp256k1, the scheme every Decentraland auth-chain verifier
//! recovers against.

use anyhow::{Context, Result, anyhow};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

const EIP191_PREFIX: &str = "\x19Ethereum Signed Message:\n";
/// Ethereum convention: recovery id 0/1 is transmitted as 27/28.
const V_OFFSET: u8 = 27;

pub fn personal_sign(private_key: &[u8; 32], message: &str) -> Result<String> {
    let key = SigningKey::from_slice(private_key).context("Invalid secp256k1 private key")?;
    let digest = eip191_digest(message);
    let (signature, recovery_id): (Signature, RecoveryId) = key
        .sign_prehash_recoverable(&digest)
        .context("Cannot sign the payload")?;
    let v = V_OFFSET.saturating_add(recovery_id.to_byte());
    Ok(format!("0x{}{v:02x}", hex::encode(signature.to_bytes())))
}

/// Lower-case `0x` address of the key's public key.
pub fn address_of(private_key: &[u8; 32]) -> Result<String> {
    let key = SigningKey::from_slice(private_key).context("Invalid secp256k1 private key")?;
    Ok(address_of_verifying_key(key.verifying_key()))
}

/// Recovers the signer address of a `personal_sign` signature.
pub fn recover_address(message: &str, signature_hex: &str) -> Result<String> {
    let raw = hex::decode(signature_hex.strip_prefix("0x").unwrap_or(signature_hex))
        .context("Signature is not hex")?;
    let (rs, v) = raw
        .split_last_chunk::<1>()
        .ok_or_else(|| anyhow!("Signature is empty"))?;
    let v = v.first().copied().unwrap_or_default();
    let recovery_id = RecoveryId::from_byte(v.wrapping_sub(V_OFFSET))
        .ok_or_else(|| anyhow!("Invalid recovery id {v}"))?;
    let signature = Signature::from_slice(rs).context("Invalid signature bytes")?;
    let key = VerifyingKey::recover_from_prehash(&eip191_digest(message), &signature, recovery_id)
        .context("Cannot recover the signer")?;
    Ok(address_of_verifying_key(&key))
}

fn eip191_digest(message: &str) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(EIP191_PREFIX.as_bytes());
    hasher.update(message.len().to_string().as_bytes());
    hasher.update(message.as_bytes());
    hasher.finalize().into()
}

fn address_of_verifying_key(key: &VerifyingKey) -> String {
    let point = key.to_encoded_point(false);
    let public_key = point.as_bytes().get(1..).unwrap_or_default();
    let hash: [u8; 32] = Keccak256::digest(public_key).into();
    let address = hash.get(12..).unwrap_or_default();
    format!("0x{}", hex::encode(address))
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    // Well-known test vector: the key `0x01` maps to this address.
    const KEY_ONE: [u8; 32] = {
        let mut key = [0u8; 32];
        key[31] = 1;
        key
    };
    const KEY_ONE_ADDRESS: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";

    #[test]
    fn address_derivation_matches_known_vector() {
        assert_eq!(
            address_of(&KEY_ONE).unwrap_or_else(|e| panic!("{e}")),
            KEY_ONE_ADDRESS
        );
    }

    #[test]
    fn signature_recovers_to_the_signer() {
        let message = "post:/intercom/tickets:1757500000000:{}";
        let signature = personal_sign(&KEY_ONE, message).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(signature.len(), 2 + 130);
        assert!(signature.ends_with("1b") || signature.ends_with("1c"));
        assert_eq!(
            recover_address(message, &signature).unwrap_or_else(|e| panic!("{e}")),
            KEY_ONE_ADDRESS
        );
    }

    #[test]
    fn a_different_message_does_not_recover_to_the_signer() {
        let signature = personal_sign(&KEY_ONE, "a").unwrap_or_else(|e| panic!("{e}"));
        let recovered = recover_address("b", &signature).unwrap_or_default();
        assert_ne!(recovered, KEY_ONE_ADDRESS);
    }
}
