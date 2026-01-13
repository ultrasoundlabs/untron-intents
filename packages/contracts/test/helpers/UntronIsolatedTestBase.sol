// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";

import {UntronIntents} from "../../src/UntronIntents.sol";

import {TriggerSmartContract} from "../../src/external/interfaces/ITronTxReader.sol";

import {MockERC20} from "../../src/mocks/MockERC20.sol";
import {MockForwarderPuller} from "../../src/mocks/MockForwarderPuller.sol";
import {MockTronTxReader} from "../../src/mocks/MockTronTxReader.sol";
import {MockUntronV3} from "../../src/mocks/MockUntronV3.sol";

/// @notice Test base for UntronIntents when you want to isolate it from IntentsForwarder mechanics.
/// @dev Uses {MockForwarderPuller} to simulate `pullFromReceiver` by minting to the intents contract.
abstract contract UntronIsolatedTestBase is Test {
    address internal solver;
    address internal maker;
    address internal untronOwner;

    MockERC20 internal usdt;
    MockERC20 internal usdc;

    MockTronTxReader internal tronReader;
    MockUntronV3 internal v3;
    UntronIntents internal intents;

    MockForwarderPuller internal forwarder;

    function setUp() public virtual {
        solver = makeAddr("solver");
        maker = makeAddr("maker");
        untronOwner = makeAddr("untronOwner");

        usdt = new MockERC20("USDT", "USDT", 6);
        usdc = new MockERC20("USDC", "USDC", 6);

        tronReader = new MockTronTxReader();
        v3 = new MockUntronV3(tronReader, makeAddr("tronController"), makeAddr("tronUsdt"));
        intents = new UntronIntents(untronOwner, v3, address(usdt));

        forwarder = new MockForwarderPuller();
    }

    function _setRecommendedFee(uint256 ppm, uint256 flat) internal {
        vm.prank(untronOwner);
        intents.setRecommendedIntentFee(ppm, flat);
    }

    function _mintAndApproveSolverDeposit(address solver_) internal {
        uint256 deposit = intents.INTENT_CLAIM_DEPOSIT();
        usdt.mint(solver_, deposit);
        vm.prank(solver_);
        usdt.approve(address(intents), type(uint256).max);
    }

    function _setTronTx_UsdtTransfer(address toTron, uint256 amount) internal {
        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = abi.encodeWithSelector(bytes4(keccak256("transfer(address,uint256)")), toTron, amount);
        tronReader.setTx(tx_);
    }

    function _tronAddrBytes21(address tronAddr) internal pure returns (bytes21 out) {
        uint168 packed = (uint168(uint8(0x41)) << 160) | uint168(uint160(tronAddr));
        return bytes21(packed);
    }
}
