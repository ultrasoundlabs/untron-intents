// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";

contract UntronIntentsProxy is TransparentUpgradeableProxy {
    constructor(address _logic, address initialOwner, bytes memory _data)
        payable
        TransparentUpgradeableProxy(_logic, initialOwner, _data)
    {}
}
