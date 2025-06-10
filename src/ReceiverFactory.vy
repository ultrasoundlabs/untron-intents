# pragma version 0.4.0
# @license MIT

"""
@title Untron Intents Receiver Factory
@notice A factory for deploying UntronReceiver contracts for Tron addresses.
@dev This contract is used to deploy UntronReceiver contracts for Tron addresses.
     These contracts then automatically bridge the tokens to the designated Tron addresses.
"""

from ethereum.ercs import IERC20
from lib.github.pcaversaccio.snekmate.src.snekmate.auth import ownable
from lib.github.pcaversaccio.snekmate.src.snekmate.utils import create2_address
# from src.interfaces import ReceiverFactory
from src.interfaces import UntronReceiver
from src.interfaces import UntronTransfers

initializes: ownable
# implements: ReceiverFactory
exports: ownable.transfer_ownership
exports: ownable.owner

# Address of the blueprint implementation for UntronReceiver contracts.
_receiverImplementation: immutable(address)
# Address of the UntronTransfers contract, which handles the bridging orders.
untronTransfers: public(address)
# Address of the trusted swapper.
# This can be an EOA or a smart contract (e.g., a TWAP-based swap contract or other AMM logic).
# This address is authorized to call swapForReceiver.
trustedSwapper: public(address)

# Address of the USDT token contract on the EVM chain.
usdt: public(address)
# Address of the USDC token contract on the EVM chain.
usdc: public(address)

# Event emitted when a new UntronReceiver contract is deployed.
event ReceiverDeployed:
    # The Tron address for which the receiver was deployed.
    destinationTronAddress: bytes20
    # The EVM address of the deployed UntronReceiver contract.
    receiver: address

@deploy
def __init__(__receiverImplementation: address):
    """
    @notice Contract constructor, called once at deployment.
    """
    # Initialize the Ownable component, setting the deployer as the initial owner.
    ownable.__init__()
    # Set the address of the UntronReceiver implementation contract.
    _receiverImplementation = __receiverImplementation


@external
@view
def receiverImplementation() -> address:
    """
    @notice External view function to return the address of the UntronReceiver implementation contract.
    @return address The address of the UntronReceiver implementation contract.
    """
    return _receiverImplementation


@external
def configure(untronTransfers: address, trustedSwapper: address, usdt: address, usdc: address):
    """
    @notice External function to configure critical addresses, callable only by the owner.
    @param untronTransfers Address of the UntronTransfers contract
    @param trustedSwapper Address of the trusted swapper
    @param usdt Address of the USDT token
    @param usdc Address of the USDC token
    """
    # Ensure only the owner can call this function.
    ownable._check_owner()
    # Set the address of the UntronTransfers contract.
    self.untronTransfers = untronTransfers
    # Set the address of the trusted swapper.
    self.trustedSwapper = trustedSwapper
    # Set the address of the USDT token.
    self.usdt = usdt
    # Set the address of the USDC token.
    self.usdc = usdc


@internal
@view
def _constructSwapData(amount: uint256, destinationTronAddress: bytes20) -> bytes32:
    """
    @notice Internal view function to construct the compact swap data for UntronTransfers.
    @param amount The amount of tokens to swap
    @param destinationTronAddress The Tron address to receive the swapped tokens
    @return bytes32 The constructed swap data
    """
    # Constructs a bytes32 payload for UntronTransfers compact swap functions.
    # Format: amount (first 6 bytes for input) | 0 (next 6 bytes for output, so UntronTransfers uses recommended) | destinationTronAddress (last 20 bytes).
    # Left-shift amount by 208 bits (26 bytes) to place it in the most significant part of the bytes32.
    # Convert destinationTronAddress to uint160, then to uint256, to align it for bitwise OR.
    # The output amount (middle 6 bytes) is intentionally set to 0.
    # This signals the UntronTransfers contract to use its recommended output amount.
    return convert((amount << 208) | convert(convert(destinationTronAddress, uint160), uint256), bytes32)


@internal
def _intron(destinationTronAddress: bytes20, receiverAddress: address):
    """
    @notice Internal function to process funds held by a receiver and initiate bridging via UntronTransfers.
    @param destinationTronAddress The Tron address to receive the bridged tokens
    @param receiverAddress The address of the UntronReceiver contract
    """
    # Create an UntronReceiver instance for the given receiverAddress.
    receiver: UntronReceiver = UntronReceiver(receiverAddress)
    # Call the receiver's withdraw function to get its entire USDT balance, transferring it to this factory contract.
    usdtAmount: uint256 = extcall receiver.withdraw(self.usdt)
    # Call the receiver's withdraw function to get its entire USDC balance, transferring it to this factory contract.
    usdcAmount: uint256 = extcall receiver.withdraw(self.usdc)

    # If there's a USDT balance withdrawn from the receiver:
    if usdtAmount > 0:
        # Call compactUsdt on the UntronTransfers contract to create a bridging order for the USDT.
        extcall IERC20(self.usdt).approve(self.untronTransfers, usdtAmount)
        extcall UntronTransfers(self.untronTransfers).compactUsdt(self._constructSwapData(usdtAmount, destinationTronAddress))
    # If there's a USDC balance withdrawn from the receiver:
    if usdcAmount > 0:
        # Call compactUsdc on the UntronTransfers contract to create a bridging order for the USDC.
        extcall IERC20(self.usdc).approve(self.untronTransfers, usdcAmount)
        extcall UntronTransfers(self.untronTransfers).compactUsdc(self._constructSwapData(usdcAmount, destinationTronAddress))


@external
def withdraw(destinationTronAddress: bytes20, tokens: DynArray[address, 8]):
    """
    @notice External function allowing a trustedSwapper to withdraw tokens from a receiver contract.
    @dev A trustedSwapper is supposed to be a smart contract at some point, and it will use this function to
         swap all tokens received into USDT or USDC which can then be used to initiate swaps to Tron
    @param destinationTronAddress The Tron address associated with the receiver contract
    @param tokens An array of token addresses to withdraw
    """
    # Asserts that the caller is the trustedSwapper.
    # The trustedSwapper can be an EOA or a smart contract that handles swaps in a trust-minimized way.
    assert msg.sender == self.trustedSwapper, "unauthorized"
    
    # Calculate the deterministic address for the receiver contract.
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    # If the receiver contract hasn't been deployed yet (no code at the address):
    if contract.codesize == 0:
        # Deploy the receiver contract.
        self.deploy(destinationTronAddress)

    # Create an UntronReceiver instance for the (now deployed) receiver contract.
    receiver: UntronReceiver = UntronReceiver(contract)
    # Call the receiver's withdraw function to pull any `tokens` it might already hold (normally from a user's direct deposit).
    # We will then send the tokens to the relayer.
    for token: address in tokens:
        inputAmount: uint256 = extcall receiver.withdraw(token)
        if inputAmount > 0:
            extcall IERC20(token).transfer(msg.sender, inputAmount)

@external
def intron(destinationTronAddress: bytes20):
    """
    @notice External function to initiate the bridging process for funds already in a receiver, or deploy and then bridge.
    @param destinationTronAddress The Tron address to receive the bridged tokens
    """
    # Calculate the deterministic address for the receiver contract.
    contract: address = self._generateReceiverAddress(destinationTronAddress)
    # If the receiver contract hasn't been deployed yet:
    if contract.codesize == 0:
        # Deploy the receiver contract.
        self.deploy(destinationTronAddress)

    # Call the internal _intron function to process funds in the receiver and bridge them.
    self._intron(destinationTronAddress, contract)

@internal
def deploy(destinationTronAddress: bytes20) -> address:
    """
    @notice Internal function to deploy a new UntronReceiver minimal proxy contract.
    @param destinationTronAddress The Tron address associated with the new receiver
    @return address The address of the newly deployed receiver contract
    """
    # Deploy an EIP-1167 minimal proxy pointing to self.receiverImplementation.
    # The salt is derived from the destinationTronAddress to ensure deterministic deployment unique to that Tron address.
    contract: address = create_minimal_proxy_to(_receiverImplementation, salt=convert(destinationTronAddress, bytes32))
    # Create an UntronReceiver instance for the newly deployed contract.
    receiver: UntronReceiver = UntronReceiver(contract)
    # Call the initialize function on the new receiver contract (likely to set its deployer/owner).
    extcall receiver.initialize()
    # Log an event indicating the deployment of the new receiver.
    log ReceiverDeployed(destinationTronAddress, contract)

    # Return the address of the newly deployed receiver contract.
    return contract

@internal
@view
def _generateReceiverAddress(destinationTronAddress: bytes20) -> address:
    """
    @notice Internal view function to calculate the deterministic address of an UntronReceiver contract using CREATE2.
    @param destinationTronAddress The Tron address associated with the receiver
    @return address The calculated address where the UntronReceiver will be deployed
    """
    # This function computes the address where a new UntronReceiver will be deployed
    # using CREATE2, based on the destinationTronAddress and the receiverImplementation.
    # This allows anyone to predict the receiver's address before deployment.

    # Construct the exact ERC-1167 minimal proxy bytecode that `create_minimal_proxy_to` would use.
    # This bytecode includes the address of `self.receiverImplementation`.
    # The ERC-1167 standard specifies a lean proxy contract that delegates all calls to a fixed implementation.
    # Bytecode structure:
    # Prefix (opcodes for setup and returning code)
    # + self.receiverImplementation (the address of the logic contract)
    # + Suffix (opcodes for delegation and revert)
    # Source: https://ercs.ethereum.org/ERCS/erc-1167
    init_code: Bytes[54] = concat(
        # 9-byte initialization opcode set + first 10 bytes of ERC-1167 standard proxy bytecode.
        b"\x60\x2d\x3d\x81\x60\x09\x3d\x39\xf3\x36\x3d\x3d\x37\x3d\x3d\x3d\x36\x3d\x73",
        # The 20-byte address of the receiverImplementation contract is embedded here.
        convert(_receiverImplementation, bytes20),
        # Last 15 bytes of the ERC-1167 standard proxy bytecode.
        b"\x5a\xf4\x3d\x82\x80\x3e\x90\x3d\x91\x60\x2b\x57\xfd\x5b\xf3"
    )

    # Calculate the Keccak256 hash of this initialization code.
    # This hash is used in the CREATE2 address calculation formula.
    init_code_hash: bytes32 = keccak256(init_code)

    # Compute the CREATE2 address using:
    # keccak256(0xff + sender_address + salt + keccak256(init_code))[12:]
    # Here, `_compute_address_self` uses `self` (this contract's address) as the sender_address.
    # The salt is derived from the destinationTronAddress.
    return create2_address._compute_address_self(convert(destinationTronAddress, bytes32), init_code_hash)

@external
@view
def generateReceiverAddress(destinationTronAddress: bytes20) -> address:
    """
    @notice External view function to publicly expose the receiver address generation logic.
    @param destinationTronAddress The Tron address for which to generate the receiver address
    @return address The deterministically
    """
    # Returns the deterministically calculated EVM address for an UntronReceiver
    # corresponding to the given destinationTronAddress.
    return self._generateReceiverAddress(destinationTronAddress)