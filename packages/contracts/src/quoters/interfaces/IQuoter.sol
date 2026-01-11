// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title Quoter interface
/// @notice The interface for quoters — simple view contracts that
///         are responsible for providing quotes for swaps in Intents contracts.
/// @dev Quoters are used as a “minimum output oracle” for swap execution:
/// - {IntentsForwarder} uses the quoted amount as a minimum to enforce that swap execution produced
///   at least that much output.
/// - Output produced above the quote may be rebated to the relayer/caller (depending on the forwarder logic).
///
/// Quoters are not necessarily price oracles:
/// - A quoter may be very simple (e.g., stablecoin 1:1).
/// - A quoter may incorporate fees or safety margins.
/// - A quoter may be stateful (e.g., single-use quotes).
/// @author Ultrasound Labs
interface IQuoter {
    /// @notice Returns the amount of tokenOut that can be obtained for a given amount of tokenIn.
    /// @param tokenIn The address of the input token.
    /// @param tokenOut The address of the output token.
    /// @param amountIn The amount of tokenIn to swap.
    /// @param deadline The deadline for the swap.
    /// @return amountOut The amount of tokenOut that can be obtained.
    /// @dev This function is deliberately not view in case a quoter needs to write to state,
    ///      for example, to invalidate quotes after first use.
    function quote(address tokenIn, address tokenOut, uint256 amountIn, uint256 deadline)
        external
        returns (uint256 amountOut);
}
