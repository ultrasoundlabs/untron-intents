# pragma version 0.4.0
# @license MIT

from pcaversaccio.snekmate.src.snekmate.auth import ownable
from interfaces import UntronResolver
from interfaces import ReceiverFactory

initializes: ownable
implements: UntronResolver
exports: ownable.transfer_ownership

urls: public(DynArray[String[1024], 16])
receiverFactory: public(ReceiverFactory)

@deploy
def __init__():
    ownable.__init__()

@external
def popUrl() -> String[1024]:
    ownable._check_owner()
    return self.urls.pop()

@external
def pushUrl(url: String[1024]):
    ownable._check_owner()
    self.urls.append(url)

@external
def setReceiverFactory(receiverFactory: ReceiverFactory):
    ownable._check_owner()
    self.receiverFactory = receiverFactory

@external
@view
def supportsInterface(interfaceID: bytes4) -> bool:
    if interfaceID == method_id("supportsInterface(bytes4)", output_type=bytes4):
        return True
    if interfaceID == method_id("resolve(bytes,bytes)", output_type=bytes4):
        return True
    return False

@internal
@pure
def base58IndexOf(char: uint256) -> uint256:
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
    num: uint256 = 0

    # max 35 chars length of a Tron address
    # we turn it into a 32-byte big endian decoded value
    for i: uint256 in range(length, bound=35):
        num = num * 58 + self.base58IndexOf(convert(slice(name, i, 1), uint256))

    # verify the checksum
    value: Bytes[21] = slice(convert((num >> 32) << 88, bytes32), 0, 21)
    checksum: Bytes[4] = slice(convert(num << 224, bytes32), 0, 4)

    if slice(sha256(sha256(value)), 0, 4) != checksum:
        raise "Invalid base58check checksum"

    # strip the 0x41 prefix and get the last 20 bytes of the address
    return convert(slice(value, 1, 20), bytes20)

@external
@view
def resolve(name: Bytes[64], data: Bytes[1024]) -> Bytes[32]:

    # ask to ping the relayer to bruteforce the case of the lowercased Tron address
    # (ENS normalizes all names to lowercase but we need the proper case to decode the Tron address)
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
    # ENS encodes the domains in DNS wire format, which is a set of length-prefixed strings
    subdomainLength: uint256 = convert(slice(fullDomain, 0, 1), uint256)

    # extract the subdomain from the full domain
    subdomain: Bytes[64] = slice(fullDomain, 1, subdomainLength)

    return subdomain, subdomainLength

@internal
@view
def isThisJustLowercase(string: Bytes[64], lowercasedString: Bytes[64]) -> bool:
    for i: uint256 in range(len(string), bound=64):
        leftLetter: uint256 = convert(slice(string, i, 1), uint256)
        rightLetter: uint256 = convert(slice(lowercasedString, i, 1), uint256)
        if leftLetter != rightLetter and leftLetter != rightLetter - 32:
            return False
    return True

@external
@view
def untronSubdomain(serverResponse: Bytes[64], originalDomain: Bytes[64]) -> Bytes[32]:
    
    serverTronAddress: Bytes[64] = b""
    serverTronAddressLength: uint256 = 0
    assert len(serverResponse) == len(originalDomain) and self.isThisJustLowercase(serverResponse, originalDomain), "server response is invalid"

    serverTronAddress, serverTronAddressLength = self.extractSubdomain(serverResponse)
    tronAddress: bytes20 = self.base58CheckIntoRawTronAddress(serverTronAddress, serverTronAddressLength)

    return abi_encode(staticcall self.receiverFactory.generateReceiverAddress(tronAddress))