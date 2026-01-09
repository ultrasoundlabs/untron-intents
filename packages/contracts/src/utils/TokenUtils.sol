// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IERC20} from "openzeppelin-contracts/contracts/token/ERC20/IERC20.sol";
import {Math} from "openzeppelin-contracts/contracts/utils/math/Math.sol";
import {SafeTransferLib} from "solady/utils/SafeTransferLib.sol";

/// @title TokenUtils
/// @notice Utility functions that work for both ERC20 and native tokens.
/// @dev For standard EVM chains, this library uses Solady's {SafeTransferLib} which expects
///      ERC-20 transfers/approvals to revert on failure (or return `true`).
///
///      IMPORTANT: Tron-side Untron V3 contracts intentionally do NOT use {SafeTransferLib}
///      for TRC-20 transfers, because some Tron TRC-20 tokens (notably USDT) can return `false`
///      even when the call succeeds. Tron-side contracts should use
///      `packages/contracts/src/utils/TronTokenUtils.sol` for TRC-20 transfers/approvals.
/// @author Ultrasound Labs
library TokenUtils {
    /// @notice Returns ERC20 or ETH balance of 'addr'.
    /// @param token The address of the token to query (0x00 = ETH).
    /// @param addr The address of the account to query.
    /// @return balance The balance of the token or ETH.
    function getBalanceOf(address token, address addr) internal view returns (uint256) {
        if (token == address(0)) {
            return addr.balance;
        } else {
            return IERC20(token).balanceOf(addr);
        }
    }

    /// @notice Approves a token transfer.
    /// @param token The address of the token to approve (0x00 = ETH; no-op).
    /// @param spender The address of the spender to approve.
    /// @param amount The amount to approve.
    function approve(address token, address spender, uint256 amount) internal {
        if (token != address(0)) {
            SafeTransferLib.safeApproveWithRetry({token: token, to: spender, amount: amount});
        } // Do nothing for native token.
    }

    /// @notice Sends an ERC20 or ETH transfer. For ETH, verify call success.
    /// @param token The address of the token to transfer (0x00 = ETH).
    /// @param recipient The address of the recipient.
    /// @param amount The amount to transfer.
    function transfer(address token, address payable recipient, uint256 amount) internal {
        if (token != address(0)) {
            SafeTransferLib.safeTransfer({token: token, to: recipient, amount: amount});
        } else {
            SafeTransferLib.safeTransferETH({to: recipient, amount: amount});
        }
    }

    /// @notice Does an ERC20 transferFrom. For ETH, it's a no-op.
    /// @param token The address of the token to transfer (0x00 = ETH).
    /// @param from The address of the sender.
    /// @param recipient The address of the recipient.
    /// @param amount The amount to transfer.
    function transferFrom(address token, address from, address payable recipient, uint256 amount) internal {
        if (token != address(0)) {
            // TokenUtils is a library agnostic to who we're transferring from.
            // Ensuring that 'from' is 'msg.sender' or acknowledging not doing it
            // is the responsibility of the caller.
            // slither-disable-next-line arbitrary-send-erc20
            SafeTransferLib.safeTransferFrom({token: token, from: from, to: recipient, amount: amount});
        }
    }

    /// @notice Full-precision mulDiv: floor(x * y / denominator).
    /// @param x The first operand.
    /// @param y The second operand.
    /// @param denominator The denominator.
    /// @return The result of the multiplication and division.
    /// @dev    Thin wrapper over OpenZeppelin Math.mulDiv for consistency across codebase.
    function mulDiv(uint256 x, uint256 y, uint256 denominator) internal pure returns (uint256) {
        return Math.mulDiv(x, y, denominator);
    }
}
