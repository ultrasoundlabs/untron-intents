use rayon::prelude::*;
use sha2::{Sha256, Digest};
use std::env;

// The Bitcoin Base58 alphabet (note that 0, O, I, and l are omitted)
const BASE58_ALPHABET: &[u8] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

// Invalid marker for Base58 lookup table
const INVALID: u8 = 255;

// Precomputed lookup table for Base58 decoding
static BASE58_LOOKUP: [u8; 128] = {
    let mut table = [INVALID; 128];
    let mut i = 0;
    while i < BASE58_ALPHABET.len() {
        table[BASE58_ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// Decodes a Base58 string into bytes and validates if it's a valid address.
/// Returns None if any character is invalid or if the decoded bytes don't form a valid address.
#[inline]
fn base58_decode_and_validate(s: &str) -> Option<Vec<u8>> {
    // Count leading '1's
    let n_zeros = s.bytes().take_while(|&b| b == b'1').count();

    // Use a larger buffer for intermediate calculations (34 bytes should be enough)
    let mut num = [0u8; 34];
    let mut num_len = 1;

    for byte in s.bytes() {
        // Fast lookup using precomputed table
        let digit = if (byte as usize) < BASE58_LOOKUP.len() {
            BASE58_LOOKUP[byte as usize]
        } else {
            return None;
        };

        if digit == INVALID {
            return None;
        }

        // Optimized big number arithmetic with fixed buffer
        let mut carry = digit as u32;
        for i in (0..num_len).rev() {
            let v = (num[i] as u32) * 58 + carry;
            num[i] = (v & 0xff) as u8;
            carry = v >> 8;
        }

        while carry > 0 && num_len < num.len() {
            num.rotate_right(1);
            num[0] = carry as u8;
            num_len += 1;
            carry = 0;
        }
    }

    // Trim leading zeros from the calculated number
    let mut start_idx = 0;
    while start_idx < num_len && num[start_idx] == 0 {
        start_idx += 1;
    }

    // Check if we have exactly 25 bytes after trimming zeros
    if num_len - start_idx != 25 {
        return None;
    }

    // Check for valid address prefix
    if num[start_idx] != 0x41 {
        return None;
    }

    // Prepare final result with leading zeros
    let mut result = vec![0u8; n_zeros];
    result.extend_from_slice(&num[start_idx..num_len]);

    // Validate checksum
    if Sha256::digest(&Sha256::digest(&result[..21]))[..4] == result[21..] {
        Some(result)
    } else {
        None
    }
}

/// Generate a single candidate with the given bitmask
#[inline]
fn generate_candidate_with_mask(input: &str, mask: usize) -> String {
    let mut letter_idx = 0;
    input.chars()
        .map(|c| {
            if c.is_alphabetic() {
                let res = if (mask >> letter_idx) & 1 == 1 {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                };
                letter_idx += 1;
                res
            } else {
                c
            }
        })
        .collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <ambiguous-base58-string>", args[0]);
        std::process::exit(1);
    }

    let input = &args[1];

    // Count letters for mask generation
    let num_letters = input.chars().filter(|c| c.is_alphabetic()).count();
    let total = 1 << num_letters;

    // Lazily generate and process candidates in parallel
    if let Some((cand, _)) = (0..total)
        .into_par_iter()
        .find_map_any(|mask| {
            let candidate = generate_candidate_with_mask(input, mask);
            if let Some(decoded) = base58_decode_and_validate(&candidate) {
                Some((candidate, decoded))
            } else {
                None
            }
        })
    {
        println!("{}", cand);
    }
}