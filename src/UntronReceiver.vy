# pragma version 0.4.0
# @license MIT

from ethereum.ercs import IERC20
from interfaces import UntronReceiver
from interfaces.external import UntronTransfers
from interfaces.external import DaimoFlexSwapper

implements: UntronReceiver

initialized: bool
destinationTronAddress: public(bytes20)
deployer: address # not ownable bc cleaner

@external
def initialize(destinationTronAddress: bytes20):
    assert not self.initialized, "already initialized"
    self.deployer = msg.sender
    self.destinationTronAddress = destinationTronAddress
    self.initialized = True

@internal
@view
def _onlyDeployer():
    assert msg.sender == self.deployer, "unauthorized"

@external
def swapIntoUsdc(flexSwapper: address, _token: address, usdc: address, extraData: Bytes[16384]):
    self._onlyDeployer()
    token: IERC20 = IERC20(_token)
    
    if _token != empty(address):
        _balance: uint256 = staticcall token.balanceOf(self)
        extcall token.approve(flexSwapper, _balance)
        extcall DaimoFlexSwapper(flexSwapper).swapToCoin(_token, _balance, usdc, extraData) 
    else:
        extcall DaimoFlexSwapper(flexSwapper).swapToCoin(_token, self.balance, usdc, extraData, value=self.balance)
    

@internal
@view
def _constructSwapData(amount: uint256) -> bytes32:
    # output amount (6-12th bytes) is 0 so that Untron Transfers contract uses the recommended one
    return convert((amount << 208) | convert(convert(self.destinationTronAddress, uint160), uint256), bytes32)

@external
def intron(_usdt: address, _usdc: address, untronTransfers: address):
    self._onlyDeployer()

    usdt: IERC20 = IERC20(_usdt)
    usdc: IERC20 = IERC20(_usdc)

    usdtBalance: uint256 = staticcall usdt.balanceOf(self)
    if usdtBalance > 0:
        extcall usdt.approve(untronTransfers, usdtBalance)
        swapData: bytes32 = self._constructSwapData(usdtBalance)
        extcall UntronTransfers(untronTransfers).compactUsdt(swapData)

    usdcBalance: uint256 = staticcall usdc.balanceOf(self)
    if usdcBalance > 0:
        extcall usdc.approve(untronTransfers, usdcBalance)
        swapData: bytes32 = self._constructSwapData(usdcBalance)
        extcall UntronTransfers(untronTransfers).compactUsdc(swapData)
