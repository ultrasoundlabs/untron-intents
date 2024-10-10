// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "forge-std/console.sol";
import "forge-std/Script.sol";
import "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import "../src/chains/MockUntronIntents.sol";
import "../src/UntronIntentsProxy.sol";

// Deploy to test locally with anvil
// Use the following command:
// forge create --rpc-url http://localhost:8545 --private-key <private-key> 
// src/chains/MockUntronIntents.sol: MockUntronIntents 
contract DeployUntronIntents is Script {
    UntronIntentsProxy proxy;
    MockUntronIntents wrappedProxyV1;

    function run() public {
        UntronIntents implementationV1 = new MockUntronIntents();
        
        // deploy proxy contract and point it to implementation
        proxy = new UntronIntentsProxy(address(implementationV1), address(this), "");
        
        // wrap in ABI to support easier calls
        wrappedProxyV1 = MockUntronIntents(address(proxy));
        wrappedProxyV1.initialize(address(0x0));

        console.log("UntronIntentsProxy deployed at address: ", address(proxy));
.    }
}