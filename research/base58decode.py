from base58 import b58decode

def base58_decode(s):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    base = len(alphabet)
    num = 0
    
    for char in s:
        print(alphabet.index(char))
        num = num * base + alphabet.index(char)
    
    print(num.to_bytes(32, 'big'))
    
    # Convert number to bytes
    byte_array = bytearray()
    while num > 0:
        byte_array.append(num % 256)
        num //= 256
    byte_array.reverse()
    
    # Handle leading zeros
    n_pad = len(s) - len(s.lstrip('1'))
    return b'\x00' * n_pad + bytes(byte_array)

# Example usage:
encoded_str = "TEVr7jCiRofduU2wtQsMWLBr1m132A3S5j"
decoded_bytes = base58_decode(encoded_str)
canonical_decoded_bytes = b58decode(encoded_str)
print(decoded_bytes)
print(canonical_decoded_bytes)
assert decoded_bytes == canonical_decoded_bytes
