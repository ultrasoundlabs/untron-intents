use rayon::prelude::*;
use sha2::{Sha256, Digest};
use std::env;

// The Bitcoin Base58 alphabet (note that 0, O, I, and l are omitted)
const BASE58_ALPHABET: &[u8] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

// Invalid marker for Base58 lookup table
const BASE58_DECODED_LEN: usize = 25;
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

/// Decodes a Base58 string into bytes and validates if it's a valid address,
/// applying case transformations based on the provided mask.
#[inline(always)]
fn base58_decode_and_validate_with_mask(input: &str, mask: usize) -> bool {
    let mut num = [0u8; 34];
    let mut offset = 33;
    let mut letter_idx = 0;
    const LOOKUP_LEN: usize = 128;

    // Initialize first digit at the end
    num[offset] = 0;

    for byte in input.bytes() {
        let candidate = if byte.is_ascii_alphabetic() {
            let bit = (mask >> letter_idx) & 1;
            letter_idx += 1;
            if bit == 1 {
                byte & !0x20 // Force uppercase
            } else {
                byte | 0x20 // Force lowercase
            }
        } else {
            byte
        };

        let digit = if (candidate as usize) < LOOKUP_LEN {
            BASE58_LOOKUP[candidate as usize]
        } else {
            return false;
        };

        if digit == INVALID {
            return false;
        }

        let mut carry = digit as u64;
        // Process from the end of the number to the current offset
        for slot in num[offset..].iter_mut().rev() {
            let v = (*slot as u64) * 58 + carry;
            *slot = (v & 0xff) as u8;
            carry = v >> 8;
        }

        if carry > 0 && offset > 0 {
            offset -= 1;
            num[offset] = carry as u8;
        }
    }

    // Skip leading zeros
    while offset < num.len() - 1 && num[offset] == 0 {
        offset += 1;
    }

    let remaining_len = num.len() - offset;
    if remaining_len != BASE58_DECODED_LEN {
        return false;
    }

    if num[offset] != 0x41 {
        return false;
    }

    let mut result = [0u8; BASE58_DECODED_LEN];
    result.copy_from_slice(&num[offset..offset + BASE58_DECODED_LEN]);

    Sha256::digest(&Sha256::digest(&result[..21]))[..4] == result[21..]
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

    let result = (0..total)
        .into_par_iter()
        .find_any(|&mask| base58_decode_and_validate_with_mask(input, mask));

    if let Some(valid_mask) = result {
        let correct_case: String = input.chars().enumerate().map(|(i, c)| {
            if c.is_alphabetic() {
                let letter_idx = input[..i].chars().filter(|c| c.is_alphabetic()).count();
                let bit = (valid_mask >> letter_idx) & 1;
                if bit == 1 {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            } else {
                c
            }
        }).collect();
        
        println!("{}", correct_case);
    } else {
        eprintln!("No valid candidate found");
    }
}