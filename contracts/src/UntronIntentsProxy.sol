// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";

/// @title Proxy contract for the UntronIntents contract
/// @author Ultrasound Labs
/// @dev This contract is a wrapper around the TransparentUpgradeableProxy contract
///      to allow for easy upgrades of the UntronIntents contract
contract UntronIntentsProxy is TransparentUpgradeableProxy {
    /// @notice Constructor for the proxy contract
    /// @param _logic The address of the implementation contract
    /// @param initialOwner The address of the initial owner
    /// @param _data The data to pass to the implementation contract
    constructor(address _logic, address initialOwner, bytes memory _data)
        payable
        TransparentUpgradeableProxy(_logic, initialOwner, _data)
    {}
}
