// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title Bridger interface
/// @notice The interface for bridgers — smart contracts responsible for bridging tokens between chains.
/// @dev Bridgers are deliberately modular:
/// - {IntentsForwarder} selects a bridger based on the output token and forwards `extraData`.
/// - Bridger implementations typically restrict `bridge()` to a single authorized caller (the forwarder)
///   to prevent arbitrary users from pulling funds out of the forwarder.
///
/// Safety note on `extraData`:
/// - `extraData` is supplied by permissionless relayers calling the forwarder.
/// - Bridgers MUST treat `extraData` as untrusted input and parse it defensively.
/// - In particular, malformed `extraData` should not allow theft, nor should it reliably “brick”
///   bridging by triggering persistent invalid state.
/// @author Ultrasound Labs
interface IBridger {
    /// @notice Bridges tokens from one chain to another.
    /// @param inputToken The address of the input token.
    /// @param inputAmount The amount of input token to bridge.
    /// @param outputAddress The address of the recipient on the destination chain.
    /// @param outputChainId The ID of the destination chain.
    /// @param extraData Additional data required for the bridge operation.
    ///                  IMPORTANT: extraData is specified arbitrarily by permissionless relayers.
    ///                  Thus, bridgers should be designed in a way that prevents invalid extraData
    ///                  from invalidating bridge operations or stealing funds.
    /// @return expectedAmountOut The amount of output token expected to be delivered on the destination.
    /// @dev The forwarder may compare `expectedAmountOut` against its own accounting to enforce
    ///      exact forwarding (e.g., revert if the bridge fee model would reduce the delivered amount).
    function bridge(
        address inputToken,
        uint256 inputAmount,
        address outputAddress,
        uint256 outputChainId,
        bytes calldata extraData
    ) external payable returns (uint256 expectedAmountOut);
}
