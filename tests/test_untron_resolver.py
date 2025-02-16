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

def test_base58decode(untron_resolver_contract):
    address = b'A1\xab\xf6\xca,\xd3\x95f\xad\x8a"\x159\xc87P\x93\xd9\x8e\x97\x04&\xc5\x96'
    canonical_encoding = base58_encode(address).encode()
    result = untron_resolver_contract.internal.base58CheckIntoRawTronAddress(canonical_encoding, len(canonical_encoding))
    print(address[1:-4], canonical_encoding, result)
    assert result == address[1:-4]

def test_lowercase_string(untron_resolver_contract):
    string = b"HelLo"
    lowercased_string = string.lower()
    result = untron_resolver_contract.internal.isThisJustLowercase(string, lowercased_string)
    assert result

# # will revert bc of CCIP-Read
# def test_resolve(untron_resolver_contract):
#     name = b"test.untron.eth"
#     data = b"test"
#     result = untron_resolver_contract.resolve(name, data)
#     assert result == b"test"

def test_extract_subdomain(untron_resolver_contract):
    full_domain = bytes.fromhex("2254455672376a4369526f6664755532777451734d574c4272316d313332413353356a06746f74726f6e0365746800")
    subdomain, subdomain_length = untron_resolver_contract.internal.extractSubdomain(full_domain)
    assert subdomain == b"TEVr7jCiRofduU2wtQsMWLBr1m132A3S5j"
    assert subdomain_length == 34

def test_untron_subdomain(untron_resolver_contract):
    result = untron_resolver_contract.untronSubdomain(b'"TEVr7jCiRofduU2wtQsMWLBr1m132A3S5j\x06totron\x03eth\x00', b'"tevr7jcirofduu2wtqsmwlbr1m132a3s5j\x06totron\x03eth\x00')
    print(result)