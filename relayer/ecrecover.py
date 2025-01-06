from eth_keys.datatypes import PrivateKey, Signature

def is_valid_signature(signature_hex: str, private_key_hex: str, message_hash: bytes) -> bool:
    """
    Checks if the given signature is valid for the provided private key.
    
    Args:
        signature_hex (str): The signature in hex format (0x prefixed)
        private_key_hex (str): The private key in hex format (0x prefixed)
        message_hash (bytes): The message hash to check against
    Returns:
        bool: True if the signature is valid, False otherwise
    
    Raises:
        ValueError: If signature or private key format is invalid
    """
    # Remove '0x' prefix if present
    signature_hex = signature_hex.removeprefix('0x')
    private_key_hex = private_key_hex.removeprefix('0x')
    
    # Convert hex strings to bytes
    signature_bytes = bytes.fromhex(signature_hex)
    signature_bytes = signature_bytes[0:64] + bytes([signature_bytes[64] % 27])
    private_key_bytes = bytes.fromhex(private_key_hex)
    
    # Create Signature and PrivateKey objects
    signature = Signature(signature_bytes)
    private_key = PrivateKey(private_key_bytes)
    public_key = private_key.public_key
    
    try:
        # Recover the public key from the signature
        recovered_public_key = signature.recover_public_key_from_msg_hash(message_hash)
        
        # Check if the recovered public key matches the one derived from the private key
        return recovered_public_key == public_key
    except Exception:
        return False

# Example usage
if __name__ == "__main__":
    # Example signature and private key (replace with actual values)
    signature = "0x255f049f63ffc43b49241aa7cf0e3be0fa085926bad75b8f835ac4f8ba5161e45cc587fcc82eed2faef92587f7f4c88cb56a96d81aa441b0ec7993cd65d46c441b"  # 65 bytes (r + s + v)
    private_key = "0xf4fff81d092bb40e7072cfaff00379805ce26f7748ea5ad9abac67476dda6255"  # 32 bytes
    message_hash = bytes.fromhex("81fc40c95d1d92e187fc2fb3ac65b248fdef600b2f786d53bf99b67e1714afce")

    try:
        print(is_valid_signature(signature, private_key, message_hash))
    except ValueError as e:
        print(f"Error: {e}")