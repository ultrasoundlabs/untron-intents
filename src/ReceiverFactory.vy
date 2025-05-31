# pragma version 0.4.0
# @license MIT

from ethereum.ercs import IERC20
from lib.github.pcaversaccio.snekmate.src.snekmate.auth import ownable
from lib.github.pcaversaccio.snekmate.src.snekmate.utils import create2_address
from src.interfaces import ReceiverFactory
from src.interfaces import UntronReceiver
from src.interfaces import UntronTransfers

initializes: ownable
implements: ReceiverFactory
exports: ownable.transfer_ownership
exports: ownable.owner

receiverImplementation: public(address)
untronTransfers: public(address)
trustedSwapper: public(address)

usdt: public(address)
usdc: public(address)

event ReceiverDeployed:
    destinationTronAddress: bytes20
    receiver: address

@deploy
def __init__():
    ownable.__init__()

@external
def configure(receiverImplementation: address, untronTransfers: address, trustedSwapper: address, usdt: address, usdc: address):
    ownable._check_owner()
    self.receiverImplementation = receiverImplementation
    self.untronTransfers = untronTransfers
    self.trustedSwapper = trustedSwapper
    self.usdt = usdt
    self.usdc = usdc

@internal
@view
def _constructSwapData(amount: uint256, destinationTronAddress: bytes20) -> bytes32:
    # output amount (6-12th bytes) is 0 so that Untron Transfers contract uses the recommended one
    return convert((amount << 208) | convert(convert(destinationTronAddress, uint160), uint256), bytes32)

@internal
def _intron(destinationTronAddress: bytes20, receiverAddress: address):
    receiver: UntronReceiver = UntronReceiver(receiverAddress)
    usdtAmount: uint256 = extcall receiver.withdraw(self.usdt)
    usdcAmount: uint256 = extcall receiver.withdraw(self.usdc)

    if usdtAmount > 0:
        extcall UntronTransfers(self.untronTransfers).compactUsdt(self._constructSwapData(usdtAmount, destinationTronAddress))
    if usdcAmount > 0:
        extcall UntronTransfers(self.untronTransfers).compactUsdc(self._constructSwapData(usdcAmount, destinationTronAddress))

@external
def swapForReceiver(destinationTronAddress: bytes20, inputToken: address, forUsdc: bool, outputAmount: uint256, intronAfter: bool):
    assert msg.sender == self.trustedSwapper, "unauthorized"
    
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    if contract.codesize == 0:
        self.deploy(destinationTronAddress)

    receiver: UntronReceiver = UntronReceiver(contract)
    extcall receiver.withdraw(inputToken)

    outputToken: address = self.usdc if forUsdc else self.usdt
    extcall IERC20(outputToken).transferFrom(msg.sender, contract, outputAmount)

    if intronAfter:
        self._intron(destinationTronAddress, contract)

@external
def intron(destinationTronAddress: bytes20):
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    if contract.codesize == 0:
        self.deploy(destinationTronAddress)

    self._intron(destinationTronAddress, contract)

@internal
def deploy(destinationTronAddress: bytes20) -> address:
    contract: address = create_minimal_proxy_to(self.receiverImplementation, salt=convert(destinationTronAddress, bytes32))
    receiver: UntronReceiver = UntronReceiver(contract)
    extcall receiver.initialize()
    log ReceiverDeployed(destinationTronAddress, contract)

    return contract

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