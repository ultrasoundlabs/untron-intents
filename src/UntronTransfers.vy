# pragma version 0.4.0
# @license MIT

"""
@title Untron Transfers
@notice An intent-based bridge for sending ERC20 tokens as USDT on Tron
@dev This version of the contract uses a trusted relayer to handle the swaps.
     In the future versions, Untron's ZK engine will be used for permissionless solving.
"""

from ethereum.ercs import IERC20
from lib.github.pcaversaccio.snekmate.src.snekmate.auth import ownable

initializes: ownable
exports: ownable.transfer_ownership
exports: ownable.owner

# Order struct for tracking yet unfilled swap orders
struct Order:
    # Address of the refund beneficiary.
    # This is the address that will receive the funds
    # if the order is not filled before the deadline.
    refundBeneficiary: address

    # Address of the token to swap from.
    # Must be a deployed ERC20 token on the deployed chain.
    token: address

    # Amount of the token to swap from.
    inputAmount: uint256

    # Tron address to receive Tron USDT.
    to: bytes20

    # Amount of the Tron USDT to receive.
    outputAmount: uint256

    # Deadline for the order.
    # If the order is not filled before the deadline,
    # the funds can be refunded to the refundBeneficiary.
    deadline: uint256

    # Address that will receive a portion of the input tokens
    referrer: address

    # How much of the input tokens will be sent to the referrer.
    # The referrer will receive a portion of the input tokens
    # as compensation for bringing the user (refundBeneficiary) to the platform.
    # The referrer fee is deducted from the input amount.
    referrerFee: uint256

# Event for when an order is created
# Listened to by the relayers to track new orders
event OrderCreated:
    # ID of the created order
    orderId: bytes32
    # Address of the refund beneficiary.
    # This is the address that will receive the funds
    # if the order is not filled before the deadline.
    refundBeneficiary: address

    # Address of the token to swap from.
    # Must be a deployed ERC20 token on the deployed chain.
    token: address

    # Amount of the token to swap from.
    inputAmount: uint256

    # Tron address to receive Tron USDT.
    to: bytes20

    # Amount of the Tron USDT to receive.
    outputAmount: uint256

    # Deadline for the order.
    # If the order is not filled before the deadline,
    # the funds can be refunded to the refundBeneficiary.
    deadline: uint256

    # Address that will receive a portion of the input tokens
    referrer: address

    # How much of the input tokens will be sent to the referrer.
    # The referrer will receive a portion of the input tokens
    # as compensation for bringing the user (refundBeneficiary) to the platform.
    # The referrer fee is deducted from the input amount.
    referrerFee: uint256

# Event for when an order is filled or cancelled
# Listened to by the relayers to track orders that are no longer active
event OrderCleared:
    # ID of the order that is no longer active
    orderId: bytes32

# Addresses of USDT and USDC tokens on the deployed chain.
# They're used in compactUsdt/compactUsdc functions
# to provide a data-saving interface for efficient swaps on the L2s.
usdt: immutable(address)
usdc: immutable(address)

# Mapping of order IDs to orders.
# Only contains orders that have not been filled yet.
orders: public(HashMap[bytes32, Order])

# Mapping of addresses to their nonce.
# Used to generate unique order IDs.
nonces: public(HashMap[address, uint256])

# Mapping of addresses to their referrer.
# Used to track the referrer of the initiator.
referrers: public(HashMap[address, address])

# System-wide referrer fee.
referrerFee: public(uint256)

# Address of the trusted relayer.
# Only this address can call claim() to fill orders.
# This can be an EOA or a smart contract (e.g., a ZK proof verifier for trust-minimized relaying).
trustedRelayer: public(address)

recommendedFixedFee: uint256
recommendedPercentFee: uint256

@external
def setReferrer(user: address, referrer: address):
    """
    @notice Sets a new referrer for a user.
    @param user The address of the user to set a referrer for.
    @param referrer The new referrer address.
    @dev If not specified, the owner will be used as the referrer.
    """
    assert msg.sender == ownable.owner
    self.referrers[user] = referrer

@external
def configure(newRelayer: address, fixedFee: uint256, percentFee: uint256, referrerFee: uint256):
    """
    @notice Configures contract parameters.
    @param newRelayer The new relayer address.
    @param fixedFee The fixed fee amount.
    @param percentFee The percentage fee (in basis points).
    @param referrerFee The referrer fee amount.
    @dev This function is used to configure the contract parameters.
         Only the current owner can call this function.
         The newRelayer can be an EOA or a smart contract designed for trust-minimized relaying.
    """
    # Verify the caller is the current owner
    assert msg.sender == ownable.owner
    
    # Update the trusted relayer address
    self.trustedRelayer = newRelayer
    
    # Update the recommended fees
    self.recommendedFixedFee = fixedFee
    self.recommendedPercentFee = percentFee

    # Update the referrer fee
    self.referrerFee = referrerFee

@internal
@view
def _recommendedOutputAmount(inputAmount: uint256) -> uint256:
    """
    @notice Calculates the recommended output amount for a given input amount.
    @param inputAmount The input amount.
    @return The recommended output amount.
    """
    return inputAmount * (10000 - self.recommendedPercentFee) // 10000 - self.recommendedFixedFee

@external
@view
def recommendedOutputAmount(inputAmount: uint256) -> uint256:
    """
    @notice Calculates the recommended output amount for a given input amount.
    @param inputAmount The input amount.
    @return The recommended output amount.
    """
    return self._recommendedOutputAmount(inputAmount)

@deploy
def __init__(_usdt: address, _usdc: address):
    """
    @notice Initializes the contract with USDT and USDC addresses and the trusted relayer.
    @param _usdt Address of the USDT token.
    @param _usdc Address of the USDC token.
    """
    # Store the USDT token address as an immutable
    # USDT address needs to be stored immutably to enable data-efficient compact swaps from USDT
    usdt = _usdt
    # Store the USDC token address as an immutable
    # USDC address needs to be stored immutably to enable data-efficient compact swaps from USDC
    usdc = _usdc
    # Initialize the ownable contract
    # This sets the initial owner to the deployer
    ownable.__init__()
    # Set the initial trusted relayer address
    # A trusted relayer is the resolver of orders
    self.trustedRelayer = ownable.owner
@internal
def _orderId(creator: address, nonce: uint256) -> bytes32:
    """
    @notice Generates a unique order ID for a given creator and nonce.
    @param creator The address of the order creator.
    @param nonce The nonce (creator's order counter) for uniqueness.
    @return The unique order ID as a bytes32 hash.
    """
    # Combine chain ID, contract address, creator address and nonce into a unique hash
    # This prevents order ID collisions across different chains and spoke pools
    return sha256(abi_encode(chain.id, self, creator, nonce))

@external
def cancel(orderId: bytes32):
    """
    @notice Cancels an expired order.
    @param orderId The ID of the order to cancel.
    @dev This function is used by the initiator of the order
         to cancel it if it expires.
    """
    # Retrieve the order from storage
    # Need to access order details to verify cancellation conditions
    order: Order = self.orders[orderId]
    # Verify the order has expired
    # Orders can only be cancelled after their deadline to prevent premature cancellations
    assert order.deadline < block.timestamp
    # Return the tokens to the refund beneficiary
    # Tokens must be returned to the beneficiary when order is cancelled
    extcall IERC20(order.token).transfer(order.refundBeneficiary, order.inputAmount)
    # Clear the order from storage
    # Cancelled orders must be removed to prevent double-spending
    # and to save storage space
    self.orders[orderId] = empty(Order)

    # Log the order as cleared
    # This is used by the relayers to track orders that are no longer active
    log OrderCleared(orderId)

@external
def claim(orderId: bytes32):
    """
    @notice Claims funds for a filled order.
    @param orderId The ID of the order to claim.
    @dev This function is used by the trusted relayer.
         The trusted relayer can be an EOA or a smart contract (e.g., a ZK proof verifier).
    """
    # Verify the caller is the trusted relayer
    # Only the trusted relayer (or a contract it controls) can claim funds for cross-chain swaps
    assert msg.sender == self.trustedRelayer

    # Retrieve the order from storage
    # Need to access order details to process the claim
    order: Order = self.orders[orderId]
    # Verify the order has expired
    # Orders can only be claimed before their deadline,
    # otherwise they can only be refunded to the initiator
    assert order.deadline >= block.timestamp
    
    # We prioritize the relayer getting the fees over the referrer
    # If the referrer fee is greater than the input amount,
    # we set the referrer fee to 0
    referrerFee: uint256 = order.referrerFee
    if referrerFee > order.inputAmount:
        referrerFee = 0

    # Transfer the tokens to the relayer
    # Relayer receives the tokens as compensation for executing the cross-chain swap
    extcall IERC20(order.token).transfer(msg.sender, order.inputAmount - referrerFee)

    # Transfer the referrer fee to the referrer
    extcall IERC20(order.token).transfer(order.referrer, referrerFee)

    # Clear the order from storage
    # Claimed orders must be removed to prevent double-spending
    self.orders[orderId] = empty(Order)

    # Log the order as cleared
    # This is used by the relayers to track orders that are no longer active
    log OrderCleared(orderId)

@internal
def _compactSwap(token: address, swapData: bytes32) -> bytes32:
    """
    @notice An internal entry-point function for "compact" swaps.
    @param token The address of the token to swap.
    @param swapData The compressed 32-byte data for the swap.
                    It contains the input amount (6 bytes), 
                    output amount in Tron USDT (6 bytes),
                    and the recipient Tron address (20 bytes compressed).
                    Deadline is hardcoded to 1 day.
                    While 6 bytes are reasonable for 6-decimal USDT,
                    some tokens, especially 18-decimal ones,
                    might require more bytes for the input amount.
                    In this case, consider using intron() instead.
    @dev This function is used by compactSwap() function.
    """
    # Extract the input amount from the first 6 bytes
    # This would only work good for token amounts which would fit into 6 bytes.
    # This is not a problem for 6-decimal USDT, but might be for 18-decimal tokens.
    # In this case, consider using intron() instead.
    inputAmount: uint256 = convert(swapData, uint256) >> 208

    # Extract the output amount from the next 6 bytes
    # Output amount is in Tron USDT, so it's limited to ~281m USDT, which is reasonable
    outputAmount: uint256 = convert(swapData, uint256) >> 160

    # Extract the output amount from the other data
    outputAmount &= convert(max_value(uint48), uint256)

    # if output amount is 0, use recommended output amount
    if outputAmount == 0:
        outputAmount = self._recommendedOutputAmount(inputAmount)

    # Extract the Tron address from the remaining 20 bytes
    # Recipient address must be decoded from the compact format to create the order
    to: bytes20 = convert(convert(convert(swapData, uint256) << 96, bytes32), bytes20)

    # Get the referrer for the user
    # If the user has no referrer, use the owner as the referrer
    referrer: address = self.referrers[msg.sender]
    if referrer == empty(address):
        referrer = ownable.owner

    # Create an order struct with a 1-day deadline
    # It is created from the decoded compact data for data-efficient processing
    order: Order = Order(
        refundBeneficiary=msg.sender,
        token=token,
        inputAmount=inputAmount,
        to=to,
        outputAmount=outputAmount,
        deadline=block.timestamp + 86400,
        referrer=referrer,
        referrerFee=self.referrerFee
    )

    # Transfer tokens from the sender to this contract
    # The contract needs to hold the tokens until the cross-chain swap is completed
    extcall IERC20(order.token).transferFrom(msg.sender, self, order.inputAmount)

    # Generate a unique order ID using the order data and sender's nonce
    # Each order needs a unique identifier for efficient tracking and claiming in the storage
    orderId: bytes32 = self._orderId(msg.sender, self.nonces[msg.sender])

    # Store the order in the orders mapping
    # Order details must be stored to allow efficient claiming or refunding
    self.orders[orderId] = order

    # Increment the sender's nonce to prevent order ID collisions
    # Nonce must increase to ensure each order has a unique ID
    self.nonces[msg.sender] += 1

    log OrderCreated(
        orderId,
        order.refundBeneficiary,
        order.token,
        order.inputAmount,
        order.to,
        order.outputAmount,
        order.deadline,
        order.referrer,
        order.referrerFee)
    
    return orderId

@external
def compactSwap(token: address, swapData: bytes32) -> bytes32:
    """
    @notice An entry-point function for "compact" swaps.
    @param token The address of the token to swap.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the provided token and data
    # Provides a public interface for creating data-efficient swap orders with custom tokens
    return self._compactSwap(token, swapData)

@external
def compactUsdt(swapData: bytes32) -> bytes32:
    """
    @notice An entry-point function for "compact" swaps from USDT.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the USDT token address
    # Provides a data-efficient public interface specifically for USDT swaps
    return self._compactSwap(usdt, swapData)

@external
def compactUsdc(swapData: bytes32) -> bytes32:
    """
    @notice An entry-point function for "compact" swaps from USDC.
    @param swapData The compressed 32-byte data for the swap.
    @dev This function uses _compactSwap() internally.
    """
    # Call the internal compact swap function with the USDC token address
    # Provides a data-efficient public interface specifically for USDC swaps
    return self._compactSwap(usdc, swapData)