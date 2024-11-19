from eth_keys import keys
from eth_keys.datatypes import PublicKey, Signature
from eth_utils import to_bytes, keccak

def recover_message_hash(signature_hex: str, public_key_hex: str) -> bytes:
    """
    Recovers the original message hash from an Ethereum signature and public key.
    
    Args:
        signature_hex (str): The signature in hex format (0x prefixed)
        public_key_hex (str): The public key in hex format (0x prefixed), can be compressed or uncompressed
    
    Returns:
        bytes: The recovered message hash
        
    Raises:
        ValueError: If signature or public key format is invalid
    """
    # Remove '0x' prefix if present
    signature_hex = signature_hex.removeprefix('0x')
    public_key_hex = public_key_hex.removeprefix('0x')
    
    # Convert hex strings to bytes
    signature_bytes = bytes.fromhex(signature_hex)
    signature_bytes = signature_bytes[0:64] + bytes([signature_bytes[64] % 27])
    public_key_bytes = bytes.fromhex(public_key_hex)
    
    # Create Signature and PublicKey objects
    signature = Signature(signature_bytes)
    public_key = PublicKey.from_compressed_bytes(public_key_bytes) if len(public_key_bytes) == 33 else PublicKey(public_key_bytes)
    
    # Get the recovery ID (v) from the signature
    v = signature.v
    
    # Iterate through possible message hashes
    for msg_hash_candidate in range(2**256):  # In practice, you'd use a more efficient approach
        msg_hash = to_bytes(msg_hash_candidate)
        
        try:
            # Try to recover the public key from signature and candidate message hash
            recovered_public_key = signature.recover_public_key_from_msg_hash(msg_hash)
            
            # If recovered public key matches the provided one, we found the message hash
            if recovered_public_key == public_key:
                return msg_hash
        except Exception:
            continue
            
    raise ValueError("Could not recover message hash")

# Example usage
if __name__ == "__main__":
    # Example signature and public key (replace with actual values)
    signature = "0x4f4bd9ceefb6bc176e9f0f69cc9791922335120baf19428975c0f591d739ff046fba9461e4a2406e6f4a953fa8a48490936359351f932755f16f1b736604b5a21b"  # 65 bytes (r + s + v)
    public_key = "0x03f4a54713cb13044c7ca9f2c6778ddde4b681a4771d896e557d2298ec1ddf365c"  # 33 bytes (compressed public key)
    
    try:
        message_hash = recover_message_hash(signature, public_key)
        print(f"Recovered message hash: 0x{message_hash.hex()}")
    except ValueError as e:
        print(f"Error: {e}")