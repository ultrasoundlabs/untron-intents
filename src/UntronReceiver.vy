# pragma version 0.4.0
# @license MIT

from ethereum.ercs import IERC20
from src.interfaces import UntronReceiver
from src.interfaces import UntronTransfers

implements: UntronReceiver

deployer: public(address) # not ownable bc cleaner

@external
def initialize():
    self.deployer = msg.sender

# this function sends a specific token to the deployer contract (or ETH if no token is specified)
# so that the deployer contract can swap it for USDT and send to Untron Transfers contract
@external
def withdraw(_token: address) -> uint256:
    assert msg.sender == self.deployer, "unauthorized"
    token: IERC20 = IERC20(_token)
    
    _balance: uint256 = 0
    if _token != empty(address):
        _balance = staticcall token.balanceOf(self)
        extcall token.transfer(self.deployer, _balance)
    else:
        _balance = self.balance
        send(self.deployer, _balance)
    
    return _balance

@external
@payable
def __default__():
    pass # this function is to receive ETH and not just ERC20 tokens