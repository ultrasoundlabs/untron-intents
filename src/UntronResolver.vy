# pragma version 0.4.0
# @license MIT

"""
@title Untron Resolver
@notice A resolver contract for Untron ENS subdomains.
@dev This contract resolves ENS subdomains to UntronReceiver addresses for Tron addresses.
"""

from lib.github.pcaversaccio.snekmate.src.snekmate.auth import ownable
from src.interfaces import UntronResolver
from src.interfaces import ReceiverFactory

initializes: ownable
implements: UntronResolver
exports: ownable.transfer_ownership
exports: ownable.owner

# Array of URLs for off-chain lookup
urls: public(DynArray[String[1024], 16])
# Address of the ReceiverFactory contract
receiverFactory: public(ReceiverFactory)

@deploy
def __init__():
    """
    @notice Contract constructor, called once at deployment.
    """
    ownable.__init__()

@external
def popUrl() -> String[1024]:
    """
    @notice Removes and returns the last URL from the urls array.
    @dev Only callable by the contract owner.
    @return The removed URL.
    """
    ownable._check_owner()
    return self.urls.pop()

@external
def pushUrl(url: String[1024]):
    """
    @notice Adds a new URL to the urls array.
    @dev Only callable by the contract owner.
    @param url The URL to add.
    """
    ownable._check_owner()
    self.urls.append(url)

@external
def setReceiverFactory(receiverFactory: ReceiverFactory):
    """
    @notice Sets the address of the ReceiverFactory contract.
    @dev Only callable by the contract owner.
    @param receiverFactory The address of the ReceiverFactory contract.
    """
    ownable._check_owner()
    self.receiverFactory = receiverFactory

@external
@view
def supportsInterface(interfaceID: bytes4) -> bool:
    """
    @notice Checks if the contract supports a given interface.
    @param interfaceID The interface identifier, as specified in ERC-165.
    @return bool True if the contract supports the interface, false otherwise.
    """
    if interfaceID == method_id("supportsInterface(bytes4)", output_type=bytes4):
        return True
    if interfaceID == method_id("resolve(bytes,bytes)", output_type=bytes4):
        return True
    return False

@internal
@pure
def base58IndexOf(char: uint256) -> uint256:
    """
    @notice Converts a base58 character to its corresponding index.
    @param char The ASCII value of the character.
    @return The index of the character in the base58 alphabet.
    """
    if char >= 49 and char <= 57:
        return char - 49
    if char >= 65 and char <= 72:
        return char - 56
    if char >= 74 and char <= 78:
        return char - 57
    if char >= 80 and char <= 90:
        return char - 58
    if char >= 97 and char <= 107:
        return char - 64
    if char >= 109 and char <= 122:
        return char - 65
    raise "Invalid base58 character"

@internal
@pure
def base58CheckIntoRawTronAddress(name: Bytes[64], length: uint256) -> bytes20:
    """
    @notice Converts a base58check-encoded Tron address to its raw 20-byte form.
    @param name The base58check-encoded Tron address.
    @param length The length of the address string.
    @return The raw 20-byte Tron address.
    """
    num: uint256 = 0

    # Decode the base58 string into a number
    for i: uint256 in range(length, bound=35):
        num = num * 58 + self.base58IndexOf(convert(slice(name, i, 1), uint256))

    # Verify the checksum
    value: Bytes[21] = slice(convert((num >> 32) << 88, bytes32), 0, 21)
    checksum: Bytes[4] = slice(convert(num << 224, bytes32), 0, 4)

    if slice(sha256(sha256(value)), 0, 4) != checksum:
        raise "Invalid base58check checksum"

    # Return the raw 20-byte address (excluding the 0x41 prefix)
    return convert(slice(value, 1, 20), bytes20)

@external
@view
def resolve(name: Bytes[64], data: Bytes[1024]) -> Bytes[32]:
    """
    @notice Resolves an ENS name to a UntronReceiver address.
    @dev This function triggers an off-chain lookup to handle case-sensitivity of Tron addresses.
    @param name The ENS name to resolve.
    @param data Additional data (unused in this implementation).
    @return The resolved address or an off-chain lookup request.
    """
    # Trigger off-chain lookup to handle case-sensitivity of Tron addresses
    raw_revert(
        concat(
            method_id("OffchainLookup(address,string[],bytes,bytes4,bytes)", output_type=bytes4),
            abi_encode(
                self,
                self.urls,
                name,
                method_id("untronSubdomain(bytes,bytes)", output_type=bytes4),
                name
            )
        )
    )

@internal
@view
def extractSubdomain(fullDomain: Bytes[64]) -> (Bytes[64], uint256):
    """
    @notice Extracts the subdomain from a full ENS domain.
    @param fullDomain The full ENS domain in DNS wire format.
    @return The extracted subdomain and its length.
    """
    subdomainLength: uint256 = convert(slice(fullDomain, 0, 1), uint256)
    subdomain: Bytes[64] = slice(fullDomain, 1, subdomainLength)
    return subdomain, subdomainLength

@internal
@view
def isThisJustLowercase(string: Bytes[64], lowercasedString: Bytes[64]) -> bool:
    """
    @notice Checks if a string is just the lowercase version of another string.
    @param string The original string.
    @param lowercasedString The potentially lowercased string.
    @return True if lowercasedString is the lowercase version of string, false otherwise.
    """
    for i: uint256 in range(len(string), bound=64):
        leftLetter: uint256 = convert(slice(string, i, 1), uint256)
        rightLetter: uint256 = convert(slice(lowercasedString, i, 1), uint256)
        if leftLetter != rightLetter and leftLetter != rightLetter - 32:
            return False
    return True

@external
@view
def untronSubdomain(serverResponse: Bytes[64], originalDomain: Bytes[64]) -> Bytes[32]:
    """
    @notice Processes the server response to resolve a Tron address to a UntronReceiver address.
    @param serverResponse The response from the off-chain lookup server.
    @param originalDomain The original ENS domain.
    @return The ABI-encoded UntronReceiver address.
    """
    serverTronAddress: Bytes[64] = b""
    serverTronAddressLength: uint256 = 0
    assert len(serverResponse) == len(originalDomain) and self.isThisJustLowercase(serverResponse, originalDomain), "server response is invalid"

    serverTronAddress, serverTronAddressLength = self.extractSubdomain(serverResponse)
    tronAddress: bytes20 = self.base58CheckIntoRawTronAddress(serverTronAddress, serverTronAddressLength)

    return abi_encode(staticcall self.receiverFactory.generateReceiverAddress(tronAddress))