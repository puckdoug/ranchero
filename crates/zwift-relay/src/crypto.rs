// SPDX-License-Identifier: AGPL-3.0-only
//
// AES-128-GCM with a non-standard 4-byte auth tag (spec §7.3).
// Mirrors the JS calls at `zwift.mjs:1092-1106` which use Node's
// `crypto.createCipheriv('aes-128-gcm', key, iv, {authTagLength: 4})`.

use crate::CodecError;
use crate::consts::{IV_LEN, KEY_LEN};

/// AES-128-GCM with 12-byte nonce and 4-byte tag — matches the Node
/// `authTagLength: 4` configuration the Zwift game client uses.
pub type Aes128Gcm4 = aes_gcm::AesGcm<aes_gcm::aes::Aes128, typenum::U12, typenum::U4>;

/// Returns `ciphertext || tag4` (the auth tag is appended). Caller
/// supplies the AAD (typically the encoded packet header).
pub fn encrypt(
    _key: &[u8; KEY_LEN],
    _iv: &[u8; IV_LEN],
    _aad: &[u8],
    _plaintext: &[u8],
) -> Vec<u8> {
    unimplemented!("STEP-08: AES-128-GCM-4 encrypt (Aes128Gcm4 :: encrypt + appended tag)")
}

/// Decrypt `ciphertext_with_tag` (last 4 bytes are the auth tag).
/// Returns `Err(CodecError::AuthTagMismatch)` on tag failure.
pub fn decrypt(
    _key: &[u8; KEY_LEN],
    _iv: &[u8; IV_LEN],
    _aad: &[u8],
    _ciphertext_with_tag: &[u8],
) -> Result<Vec<u8>, CodecError> {
    unimplemented!("STEP-08: AES-128-GCM-4 decrypt with tag verification")
}
