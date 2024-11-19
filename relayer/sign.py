from eth_keys import keys
from eth_utils import to_bytes

def sign_message(private_key_hex: str, message_hash: str) -> str:
    """
    Signs a message hash using the provided private key.
    
    Args:
        private_key_hex (str): The private key in hex format (0x prefixed)
        message_hash (str): The message hash to sign in hex format (0x prefixed)
    Returns:
        str: The signature in hex format (0x prefixed)
    
    Raises:
        ValueError: If private key or message hash format is invalid
    """
    # Remove '0x' prefix if present
    private_key_hex = private_key_hex.removeprefix('0x')
    message_hash = message_hash.removeprefix('0x')
    
    # Convert hex strings to bytes
    private_key_bytes = bytes.fromhex(private_key_hex)
    message_hash_bytes = bytes.fromhex(message_hash)
    
    # Create PrivateKey object
    private_key = keys.PrivateKey(private_key_bytes)
    
    # Sign the message hash
    signature = private_key.sign_msg_hash(message_hash_bytes)
    
    # Convert signature to hex format
    signature_hex = "0x" + signature.r.to_bytes(32).hex() + signature.s.to_bytes(32).hex() + bytes([signature.v + 27]).hex()

    return signature_hex

# Example usage
if __name__ == "__main__":
    # Example private key and message hash (replace with actual values)
    private_key = "0xf4fff81d092bb40e7072cfaff00379805ce26f7748ea5ad9abac67476dda6255"  # 32 bytes
    message_hash = "0x81fc40c95d1d92e187fc2fb3ac65b248fdef600b2f786d53bf99b67e1714afce"

    try:
        signature = sign_message(private_key, message_hash)
        print(f"Signature: {signature}")
    except ValueError as e:
        print(f"Error: {e}")