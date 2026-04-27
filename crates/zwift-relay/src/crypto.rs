// SPDX-License-Identifier: AGPL-3.0-only
//
// AES-128-GCM with a non-standard 4-byte auth tag (spec §7.3).
// Mirrors the JS calls at `zwift.mjs:1092-1106` which use Node's
// `crypto.createCipheriv('aes-128-gcm', key, iv, {authTagLength: 4})`.
//
// We compose GCM by hand because the `aes-gcm` crate's `AesGcm`
// type seals the `TagSize` trait to RFC 5288's allowed range
// (12-16 bytes). Zwift's 4-byte tags fall outside that. The
// construction below follows NIST SP 800-38D §7 with a 12-byte IV
// and a tag truncated to `TAG_LEN` bytes; correctness is pinned by
// the Node-derived known-answer test in `tests/crypto.rs`. The
// underlying primitives (`aes::Aes128`, `ghash::GHash`) are the
// audited building blocks the `aes-gcm` crate also uses.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use ghash::GHash;
use ghash::universal_hash::UniversalHash;
use subtle::ConstantTimeEq;

use crate::CodecError;
use crate::consts::{IV_LEN, KEY_LEN, TAG_LEN};

const BLOCK_LEN: usize = 16;

/// Returns `ciphertext || tag4` (the auth tag is appended). Caller
/// supplies the AAD (typically the encoded packet header).
pub fn encrypt(key: &[u8; KEY_LEN], iv: &[u8; IV_LEN], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let ciphertext = ctr_apply(&cipher, iv, plaintext);
    let tag = compute_tag(&cipher, iv, aad, &ciphertext);
    let mut out = Vec::with_capacity(ciphertext.len() + TAG_LEN);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&tag);
    out
}

/// Decrypt `ciphertext_with_tag` (last `TAG_LEN` bytes are the auth
/// tag). Returns `Err(CodecError::AuthTagMismatch)` on tag failure.
pub fn decrypt(
    key: &[u8; KEY_LEN],
    iv: &[u8; IV_LEN],
    aad: &[u8],
    ciphertext_with_tag: &[u8],
) -> Result<Vec<u8>, CodecError> {
    if ciphertext_with_tag.len() < TAG_LEN {
        return Err(CodecError::TooShort {
            needed: TAG_LEN,
            got: ciphertext_with_tag.len(),
        });
    }
    let split = ciphertext_with_tag.len() - TAG_LEN;
    let (ciphertext, received_tag) = ciphertext_with_tag.split_at(split);

    let cipher = Aes128::new(GenericArray::from_slice(key));
    let expected_tag = compute_tag(&cipher, iv, aad, ciphertext);
    if expected_tag.ct_eq(received_tag).unwrap_u8() != 1 {
        return Err(CodecError::AuthTagMismatch);
    }
    Ok(ctr_apply(&cipher, iv, ciphertext))
}

// --- internals -----------------------------------------------------

/// Apply the GCM keystream (CTR mode starting at counter `J0 + 1`,
/// where `J0 = iv || 0^31 || 1` for a 12-byte IV).
fn ctr_apply(cipher: &Aes128, iv: &[u8; IV_LEN], data: &[u8]) -> Vec<u8> {
    let mut counter: u32 = 2; // J0 + 1
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(BLOCK_LEN) {
        let mut ks_block = counter_block(iv, counter);
        cipher.encrypt_block(&mut ks_block);
        for (k, p) in ks_block.iter().zip(chunk.iter()) {
            out.push(k ^ p);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

/// Compute the 4-byte truncated tag: `MSB_4(GHASH_H(A || pad || C ||
/// pad || len(A)_64 || len(C)_64) XOR E_K(J0))`.
fn compute_tag(cipher: &Aes128, iv: &[u8; IV_LEN], aad: &[u8], ciphertext: &[u8]) -> [u8; TAG_LEN] {
    // H = E_K(0^128) — the GHASH key.
    let mut h_block = GenericArray::default();
    cipher.encrypt_block(&mut h_block);
    let mut ghash = GHash::new(&h_block);

    feed_padded(&mut ghash, aad);
    feed_padded(&mut ghash, ciphertext);

    let mut len_block = GenericArray::default();
    len_block[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    len_block[8..].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
    ghash.update(&[len_block]);
    let s = ghash.finalize();

    // E_K(J0) where J0 = iv || 0^31 || 1.
    let mut j0 = counter_block(iv, 1);
    cipher.encrypt_block(&mut j0);

    let mut tag = [0u8; TAG_LEN];
    for i in 0..TAG_LEN {
        tag[i] = s[i] ^ j0[i];
    }
    tag
}

/// Feed `data` into GHASH in 16-byte blocks, zero-padding the last
/// block if `data.len()` isn't a multiple of 16.
fn feed_padded(ghash: &mut GHash, data: &[u8]) {
    let mut iter = data.chunks_exact(BLOCK_LEN);
    for block in &mut iter {
        ghash.update(&[GenericArray::clone_from_slice(block)]);
    }
    let tail = iter.remainder();
    if !tail.is_empty() {
        let mut padded = GenericArray::default();
        padded[..tail.len()].copy_from_slice(tail);
        ghash.update(&[padded]);
    }
}

/// Build the GCM counter block `iv || counter_be32`.
fn counter_block(iv: &[u8; IV_LEN], counter: u32) -> GenericArray<u8, aes::cipher::consts::U16> {
    let mut block = GenericArray::default();
    block[..IV_LEN].copy_from_slice(iv);
    block[IV_LEN..].copy_from_slice(&counter.to_be_bytes());
    block
}
