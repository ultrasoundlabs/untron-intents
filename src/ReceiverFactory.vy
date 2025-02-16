# pragma version 0.4.0
# @license MIT

from pcaversaccio.snekmate.src.snekmate.auth import ownable
from pcaversaccio.snekmate.src.snekmate.utils import create2_address
from interfaces import ReceiverFactory
from interfaces import UntronReceiver

initializes: ownable
implements: ReceiverFactory
exports: ownable.transfer_ownership

receiverImplementation: public(address)
flexSwapper: public(address)
untronTransfers: public(address)

usdt: public(address)
usdc: public(address)

event ReceiverDeployed:
    destinationTronAddress: bytes20
    receiver: address

@deploy
def __init__():
    ownable.__init__()

@external
def setReceiverImplementation(receiverImplementation: address):
    ownable._check_owner()
    self.receiverImplementation = receiverImplementation

@external
def setFlexSwapper(flexSwapper: address):
    ownable._check_owner()
    self.flexSwapper = flexSwapper

@external
def setUntronTransfers(untronTransfers: address):
    ownable._check_owner()
    self.untronTransfers = untronTransfers

@external
def setUsdt(usdt: address):
    ownable._check_owner()
    self.usdt = usdt

@external
def setUsdc(usdc: address):
    ownable._check_owner()
    self.usdc = usdc

@internal
def deploy(destinationTronAddress: bytes20) -> address:
    contract: address = create_minimal_proxy_to(self.receiverImplementation, salt=convert(destinationTronAddress, bytes32))
    receiver: UntronReceiver = UntronReceiver(contract)
    extcall receiver.initialize(destinationTronAddress)

    log ReceiverDeployed(destinationTronAddress, contract)

    return contract

@external
def swapIntoUsdc(destinationTronAddress: bytes20, _token: address, extraData: Bytes[16384]):
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    if contract.codesize == 0:
        self.deploy(destinationTronAddress)

    receiver: UntronReceiver = UntronReceiver(contract)
    extcall receiver.swapIntoUsdc(self.flexSwapper, _token, self.usdc, extraData)

@external
def intron(destinationTronAddress: bytes20):
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    if contract.codesize == 0:
        self.deploy(destinationTronAddress)

    receiver: UntronReceiver = UntronReceiver(contract)
    extcall receiver.intron(self.usdt, self.usdc, self.untronTransfers)

@internal
@view
def _generateReceiverAddress(destinationTronAddress: bytes20) -> address:

    # Construct the same init code used by create_minimal_proxy_to(self.implementation).
    # The standard EIP-1167 minimal proxy runtime code (with `implementation` inlined).
    # The bytecode below is exactly what Vyper uses internally for create_minimal_proxy_to.
    init_code: Bytes[54] = concat(
        # 9-byte initialization opcode set + first 10 bytes of EIP-1167
        b"\x60\x2d\x3d\x81\x60\x09\x3d\x39\xf3\x36\x3d\x3d\x37\x3d\x3d\x3d\x36\x3d\x73",
        convert(self.receiverImplementation, bytes20),      # 20 bytes for implementation address
        b"\x5a\xf4\x3d\x82\x80\x3e\x90\x3d\x91\x60\x2b\x57\xfd\x5b\xf3"  # Last 15 bytes of EIP-1167
    )

    # Get keccak256 of that init_code.
    init_code_hash: bytes32 = keccak256(init_code)

    return create2_address._compute_address_self(convert(destinationTronAddress, bytes32), init_code_hash)

@external
@view
def generateReceiverAddress(destinationTronAddress: bytes20) -> address:
    return self._generateReceiverAddress(destinationTronAddress)