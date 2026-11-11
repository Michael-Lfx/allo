//! Edge Read Aloud wire checksum, not a user or provider credential.
//! Standard SHA-256 is supplied by the workspace's existing sha2 dependency.
use sha2::{Digest, Sha256};
pub(crate) const CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
pub(crate) const CLIENT_VERSION: &str = "143.0.3650.75";

pub(crate) fn gec(unix_seconds: u64) -> String {
    let seconds = unix_seconds + 11_644_473_600;
    let ticks = (seconds - seconds % 300) * 10_000_000;
    let mut bytes = b"Copyright (C) Microsoft Corporation. All rights reserved.".to_vec();
    bytes.extend_from_slice(format!("{ticks}{CLIENT_TOKEN}").as_bytes());
    let digest = Sha256::digest(&bytes);
    let key_a = u32::from_le_bytes(digest[0..4].try_into().unwrap()) | 1;
    let key_b = u32::from_le_bytes(digest[4..8].try_into().unwrap()) | 1;
    bytes.resize(bytes.len().div_ceil(8) * 8, 0);
    let (mut swapped_last, mut swapped_sum, mut reversible, mut reversible_sum) =
        (0u32, 0u32, 0u32, 0u32);
    for pair in bytes.chunks_exact(8) {
        let x = u32::from_le_bytes(pair[..4].try_into().unwrap());
        let y = u32::from_le_bytes(pair[4..].try_into().unwrap());
        let left = x
            .rotate_left(16)
            .wrapping_mul(0xef0569fb)
            .wrapping_add(x.wrapping_mul(key_a))
            .rotate_left(16)
            .wrapping_mul(0x76f50f34);
        let right = y
            .rotate_left(16)
            .wrapping_mul(0xc31713db)
            .wrapping_add(y.wrapping_mul(key_b))
            .rotate_left(16)
            .wrapping_mul(0x1881ce3c);
        swapped_last = right
            .rotate_left(16)
            .wrapping_mul(0x35bd1ec9)
            .wrapping_add(right);
        swapped_sum = swapped_sum
            .wrapping_add(
                left.rotate_left(16)
                    .wrapping_mul(0xc3efea97)
                    .wrapping_add(left),
            )
            .wrapping_add(swapped_last);
        reversible = reversible.wrapping_add(x).wrapping_mul(key_a);
        for factor in [0xcf98b111, 0x87085b9f, 0x12ceb96d, 0x257e1d83] {
            reversible = reversible.rotate_left(16).wrapping_mul(factor);
        }
        reversible_sum = reversible_sum.wrapping_add(reversible);
        let right = reversible
            .wrapping_add(y)
            .wrapping_mul(key_b)
            .rotate_left(16)
            .wrapping_mul(0xa27416f5)
            .rotate_left(16)
            .wrapping_mul(0xd38396ff)
            .rotate_left(16);
        reversible = right
            .wrapping_mul(0x7c932b89)
            .rotate_left(16)
            .wrapping_mul(0xbfa49f69);
        reversible_sum = reversible_sum.wrapping_add(reversible);
    }
    let checksum: Vec<u8> = (swapped_last ^ reversible)
        .to_le_bytes()
        .into_iter()
        .chain((swapped_sum ^ reversible_sum).to_le_bytes())
        .collect();
    let hex = |data: &[u8]| {
        data.iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    };
    format!("{}-{}", hex(&checksum), hex(&digest))
}
