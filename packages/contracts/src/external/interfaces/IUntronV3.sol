// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ITronTxReader} from "./ITronTxReader.sol";

/// @title IUntronV3
/// @notice Interface for UntronV3 contract
/// @dev This interface defines the functions that can be called on the UntronV3 contract.
/// @author Ultrasound Labs
interface IUntronV3 {
    /// @notice Returns the address of the TronTxReader contract.
    /// @return The address of the TronTxReader contract.
    function tronReader() external view returns (ITronTxReader);

    /// @notice Returns the address of UntronController contract on Tron.
    /// @return The address of the Controller contract.
    function CONTROLLER_ADDRESS() external view returns (address);

    /// @notice Returns the address of the USDT token on Tron.
    /// @return The address of the USDT token.
    function tronUsdt() external view returns (address);
}
