import base58

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def base58_encode(data: bytes) -> str:
    """Encodes a bytes object into a Base58 string."""
    # Convert bytes to a large integer
    num = int.from_bytes(data, 'big')
    
    # Encode into Base58
    encoded = ''
    while num > 0:
        num, remainder = divmod(num, 58)
        encoded = BASE58_ALPHABET[remainder] + encoded
    
    # Handle leading zeros
    leading_zeros = len(data) - len(data.lstrip(b'\x00'))
    return '1' * leading_zeros + encoded

def test_base58_equivalence():
    test_cases = [
        b'A1\xab\xf6\xca,\xd3\x95f\xad\x8a"\x159\xc87P\x93\xd9\x8e\x97\x04&\xc5\x96',
        b"Hello, world!",
        b"1234567890",
        b"\x00\x00\x00Test",
        b"OpenAI GPT",
        b"\xff\xff\xff\xff",
        b""  # Edge case: empty input
    ]
    
    for case in test_cases:
        custom_encoded = base58_encode(case)
        lib_encoded = base58.b58encode(case).decode()
        assert custom_encoded == lib_encoded, f"Mismatch for input {case}: {custom_encoded} != {lib_encoded}"
    
    print("All tests passed!")

# Run the test
test_base58_equivalence()