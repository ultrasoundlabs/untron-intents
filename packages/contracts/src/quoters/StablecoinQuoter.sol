// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IQuoter} from "./interfaces/IQuoter.sol";

/// @title StablecoinQuoter
/// @notice Simple quoter for USD stablecoin <> USD stablecoin
///         swaps that always returns outAmount equal to inAmount.
/// @dev Intended for cases where `tokenIn` and `tokenOut` are both USD-denominated and
///      the execution path is expected to be close to 1:1 (e.g., USDC <-> USDT style swaps).
///      This provides a conservative “minimum output” of exactly the input amount.
/// @author Ultrasound Labs
contract StablecoinQuoter is IQuoter {
    /// @inheritdoc IQuoter
    function quote(address, address, uint256 amountIn, uint256) external pure returns (uint256 amountOut) {
        return amountIn;
    }
}
