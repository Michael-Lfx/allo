use crate::error::UpdateError;
use minisign_verify::{PublicKey, Signature};

pub fn verify_bytes(
    pubkey_b64: &str,
    signature_b64: &str,
    bytes: &[u8],
) -> Result<(), UpdateError> {
    let public_key = PublicKey::decode(pubkey_b64)
        .map_err(|error| UpdateError::Verify(error.to_string()))?;
    let signature = Signature::decode(signature_b64)
        .map_err(|error| UpdateError::Verify(error.to_string()))?;
    public_key
        .verify(bytes, &signature, false)
        .map_err(|error| UpdateError::Verify(error.to_string()))
}
