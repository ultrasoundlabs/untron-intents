# pragma version 0.4.0
# @license MIT

"""
@title Untron Intents Receiver
@notice A receiver contract for Untron Intents.
@dev This contract is used to receive tokens from Untron Intents and bridge them to the designated Tron addresses.
"""

from ethereum.ercs import IERC20
# from src.interfaces import UntronReceiver
from src.interfaces import UntronTransfers

# implements: UntronReceiver

# Address of the deployer contract (ReceiverFactory).
# Not using Ownable for a cleaner implementation.
deployer: public(address)

@external
def initialize():
    """
    @notice Initializes the contract, setting the deployer.
    """
    self.deployer = msg.sender

@external
def withdraw(_token: address) -> uint256:
    """
    @notice Withdraws a specific token (or ETH) to the deployer contract.
    @dev The deployer contract can then swap it for USDT or USDC and send to UntronTransfers contract.
    @param _token The address of the token to withdraw (or empty address for ETH).
    @return The amount of tokens or ETH withdrawn.
    """
    # Verify that only the deployer contract can call this function
    assert msg.sender == self.deployer, "unauthorized"
    
    # Initialize balance variable to track amount being withdrawn
    _balance: uint256 = 0
    
    # If we withdraw ETH, _token is all zeros
    if _token != empty(address):
        # Create interface to interact with the token contract
        token: IERC20 = IERC20(_token)
        # Get the current token balance of this contract
        _balance = staticcall token.balanceOf(self)
        # If there are tokens to withdraw, transfer them to deployer
        if _balance > 0:
            extcall token.transfer(self.deployer, _balance)
    else:
        # For ETH withdrawal, get the contract's ETH balance
        _balance = self.balance
        # If there is ETH to withdraw, send it to deployer
        if _balance > 0:
            send(self.deployer, _balance)
    
    # Return the amount that was withdrawn
    return _balance

@external
@payable
def __default__():
    """
    @notice Fallback function to receive ETH.
    """
    pass