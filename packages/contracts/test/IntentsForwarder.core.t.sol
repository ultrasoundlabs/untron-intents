// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IntentsForwarder} from "../src/IntentsForwarder.sol";
import {Call} from "../src/SwapExecutor.sol";

import {Vm} from "forge-std/Vm.sol";
import {IntentsForwarderIndex} from "../src/index/IntentsForwarderIndex.sol";

import {ForwarderTestBase} from "./helpers/ForwarderTestBase.sol";
import {MockERC20} from "./mocks/MockERC20.sol";
import {MockQuoter} from "./mocks/MockQuoter.sol";
import {ExactBridger, FeeBridger} from "./mocks/MockBridgers.sol";

contract IntentsForwarderCoreTest is ForwarderTestBase {
    function test_receiver_prediction_and_deploy() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 salt = baseSalt(block.chainid, beneficiary, false, bytes32(0));

        address predicted = forwarder.predictReceiverAddress(salt);
        assertEq(predicted.code.length, 0);

        vm.expectEmit(true, true, true, true, address(forwarder));
        emit IntentsForwarderIndex.ReceiverDeployed(salt, predicted, forwarder.RECEIVER_IMPLEMENTATION());
        forwarder.getReceiver(salt);
        assertGt(predicted.code.length, 0);
    }

    function test_pullReceiver_baseMode_usesReceiverBalance() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bool beneficiaryClaimOnly = false;

        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, beneficiaryClaimOnly, bytes32(0));
        address payable receiver = forwarder.predictReceiverAddress(receiverSalt);

        uint256 receiverFunds = 123e6;
        usdt.mint(address(this), receiverFunds);
        require(usdt.transfer(receiver, receiverFunds));

        assertEq(usdt.balanceOf(address(forwarder)), 0);
        assertEq(usdt.balanceOf(receiver), receiverFunds);

        bytes32 forwardSalt = bytes32(uint256(1));
        bytes32 forwardId = keccak256(
            abi.encode(
                address(forwarder),
                block.chainid,
                receiverSalt,
                forwardSalt,
                address(usdt),
                address(usdt),
                uint256(0),
                block.chainid,
                beneficiary,
                beneficiaryClaimOnly
            )
        );

        vm.recordLogs();
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: beneficiaryClaimOnly,
                intentHash: bytes32(0),
                forwardSalt: forwardSalt,
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );

        Vm.Log[] memory entries = vm.getRecordedLogs();
        bytes32 forwardCompletedSig =
            keccak256("ForwardCompleted(bytes32,bool,uint256,uint256,uint256,uint256,bool,address,uint256,bytes32)");
        bool sawForwardCompleted = false;
        for (uint256 i = 0; i < entries.length; ++i) {
            Vm.Log memory logEntry = entries[i];
            if (logEntry.emitter != address(forwarder)) continue;
            if (logEntry.topics.length == 0 || logEntry.topics[0] != forwardCompletedSig) continue;
            if (logEntry.topics.length < 2 || logEntry.topics[1] != forwardId) continue;
            sawForwardCompleted = true;
            break;
        }
        assertTrue(sawForwardCompleted);

        assertEq(usdt.balanceOf(beneficiary), receiverFunds);
        assertEq(usdt.balanceOf(receiver), 0);
    }

    function test_pullReceiver_baseMode_explicitBalance() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        uint256 pullAmount = 250e6;
        bytes32 forwardSalt = bytes32(uint256(1));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, forwardSalt, address(usdt), pullAmount);
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);

        usdt.mint(receiver, 1000e6);

        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: forwardSalt,
                balance: pullAmount,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );

        assertEq(usdt.balanceOf(beneficiary), pullAmount);
        assertEq(usdt.balanceOf(receiver), 1000e6 - pullAmount);
    }

    function test_pullReceiver_beneficiaryClaimOnly_enforced() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, true, bytes32(0));
        address payable receiver = forwarder.predictReceiverAddress(receiverSalt);

        usdt.mint(receiver, 1e6);

        vm.expectRevert(IntentsForwarder.PullerUnauthorized.selector);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: true,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(1)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );

        vm.prank(beneficiary);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: true,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(1)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
        assertEq(usdt.balanceOf(beneficiary), 1e6);
    }

    function test_pullReceiver_swap_disallowed_onEphemeral() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));

        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, bytes32(uint256(111)), address(usdc), 1e6);
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);
        usdt.mint(receiver, 1e6);

        vm.expectRevert(IntentsForwarder.SwapOnEphemeralReceiversNotAllowed.selector);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(111)),
                balance: 1e6,
                tokenIn: address(usdt),
                tokenOut: address(usdc),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
    }

    function test_pullReceiver_swap_happyPath_rebatesSurplus() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        address payable receiver = forwarder.predictReceiverAddress(receiverSalt);

        uint256 inAmount = 100e6;
        usdt.mint(receiver, inAmount);

        MockQuoter quoter = new MockQuoter();
        quoter.setAmountOut(90e6);

        vm.prank(owner);
        forwarder.setQuoter(address(usdt), quoter);

        Call[] memory swapData = new Call[](1);
        swapData[0] = Call({
            to: address(usdc), value: 0, data: abi.encodeCall(usdc.mint, (address(forwarder.SWAP_EXECUTOR()), 100e6))
        });

        address relayer = makeAddr("relayer");
        vm.prank(relayer);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(222)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdc),
                swapData: swapData,
                bridgeData: ""
            })
        );

        assertEq(usdc.balanceOf(beneficiary), 90e6);
        assertEq(usdc.balanceOf(relayer), 10e6);
    }

    function test_pullReceiver_swap_reverts_ifInsufficientOutput() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        address payable receiver = forwarder.predictReceiverAddress(receiverSalt);

        usdt.mint(receiver, 100e6);

        MockQuoter quoter = new MockQuoter();
        quoter.setAmountOut(90e6);

        vm.prank(owner);
        forwarder.setQuoter(address(usdt), quoter);

        Call[] memory swapData = new Call[](1);
        swapData[0] = Call({
            to: address(usdc), value: 0, data: abi.encodeCall(usdc.mint, (address(forwarder.SWAP_EXECUTOR()), 50e6))
        });

        vm.expectRevert();
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(1)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdc),
                swapData: swapData,
                bridgeData: ""
            })
        );
    }

    function test_pullReceiver_bridge_unsupportedOutputToken_reverts() external {
        ExactBridger bridger = new ExactBridger();
        vm.prank(owner);
        forwarder.setBridgers(bridger, bridger);

        MockERC20 other = new MockERC20("OTHER", "OTHER", 6);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(999999, beneficiary, false, bytes32(0));
        address payable receiver = forwarder.predictReceiverAddress(receiverSalt);
        other.mint(receiver, 1e6);

        vm.expectRevert(IntentsForwarder.UnsupportedOutputToken.selector);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: 999999,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(1)),
                balance: 0,
                tokenIn: address(other),
                tokenOut: address(other),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
    }

    function test_pullReceiver_bridge_usdc_refundsMsgValue() external {
        ExactBridger usdtBridger = new ExactBridger();
        ExactBridger usdcBridger = new ExactBridger();
        vm.prank(owner);
        forwarder.setBridgers(usdtBridger, usdcBridger);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(999999, beneficiary, false, bytes32(0));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, bytes32(uint256(7)), address(usdc), 5e6);
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);

        usdc.mint(receiver, 5e6);

        address relayer = makeAddr("relayer");
        vm.deal(relayer, 1 ether);
        uint256 relayerEthBefore = relayer.balance;

        vm.prank(relayer);
        forwarder.pullReceiver{value: 0.3 ether}(
            IntentsForwarder.PullRequest({
                targetChain: 999999,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(7)),
                balance: 5e6,
                tokenIn: address(usdc),
                tokenOut: address(usdc),
                swapData: new Call[](0),
                bridgeData: hex"1234"
            })
        );

        assertEq(relayer.balance, relayerEthBefore);
        assertEq(usdcBridger.lastMsgValue(), 0);
        assertEq(usdcBridger.lastInputAmount(), 5e6);
    }

    function test_pullReceiver_bridge_usdt_refundsUnusedMsgValue() external {
        ExactBridger usdtBridger = new ExactBridger();
        ExactBridger usdcBridger = new ExactBridger();
        vm.prank(owner);
        forwarder.setBridgers(usdtBridger, usdcBridger);

        usdtBridger.setRefundToCaller(0.2 ether);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(999999, beneficiary, false, bytes32(0));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, bytes32(uint256(7)), address(usdt), 5e6);
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);

        usdt.mint(receiver, 5e6);

        address relayer = makeAddr("relayer");
        vm.deal(relayer, 1 ether);
        uint256 relayerEthBefore = relayer.balance;

        vm.prank(relayer);
        forwarder.pullReceiver{value: 0.3 ether}(
            IntentsForwarder.PullRequest({
                targetChain: 999999,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(7)),
                balance: 5e6,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );

        // Bridger refunded 0.2 ETH back to the forwarder; forwarder passes it through to relayer.
        assertEq(relayer.balance, relayerEthBefore - 0.3 ether + 0.2 ether);
        assertEq(usdtBridger.lastMsgValue(), 0.3 ether);
    }

    function test_pullReceiver_bridge_reverts_ifExpectedAmountOutMismatch() external {
        FeeBridger usdtBridger = new FeeBridger();
        usdtBridger.setFee(1);

        ExactBridger usdcBridger = new ExactBridger();
        vm.prank(owner);
        forwarder.setBridgers(usdtBridger, usdcBridger);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(999999, beneficiary, false, bytes32(0));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, bytes32(uint256(7)), address(usdt), 5e6);
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);

        usdt.mint(receiver, 5e6);

        vm.expectRevert(IntentsForwarder.InsufficientOutputAmount.selector);
        forwarder.pullReceiver(
            IntentsForwarder.PullRequest({
                targetChain: 999999,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(7)),
                balance: 5e6,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
    }
}
