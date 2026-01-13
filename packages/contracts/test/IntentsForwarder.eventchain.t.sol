// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {StdAssertions} from "forge-std/StdAssertions.sol";
import {Vm} from "forge-std/Vm.sol";

import {IntentsForwarder} from "../src/IntentsForwarder.sol";
import {EventChainGenesis} from "../src/utils/EventChainGenesis.sol";
import {Call} from "../src/SwapExecutor.sol";

import {ForwarderTestBase} from "./helpers/ForwarderTestBase.sol";
import {MockERC20} from "../src/mocks/MockERC20.sol";
import {MockQuoter} from "../src/mocks/MockQuoter.sol";
import {ExactBridger} from "../src/mocks/MockBridgers.sol";
import {MockReverter} from "./mocks/MockReverter.sol";

abstract contract EventChainAsserts is StdAssertions {
    bytes32 internal constant _EVENT_APPENDED_SIG = keccak256("EventAppended(uint256,bytes32,bytes32,bytes32,bytes)");

    function _countEventAppended(Vm.Log[] memory entries, address emitter) internal pure returns (uint256 count) {
        for (uint256 i = 0; i < entries.length; ++i) {
            Vm.Log memory logEntry = entries[i];
            if (logEntry.emitter != emitter) continue;
            if (logEntry.topics.length == 0 || logEntry.topics[0] != _EVENT_APPENDED_SIG) continue;
            ++count;
        }
    }

    function _assertAndRecomputeEventChainFromLogs(
        IntentsForwarder forwarder,
        uint256 seqBefore,
        bytes32 tipBefore,
        Vm.Log[] memory entries,
        uint256 expectedAppends
    ) internal view {
        uint256 appendCount = _countEventAppended(entries, address(forwarder));
        assertEq(appendCount, expectedAppends);

        bytes32 tip = tipBefore;
        uint256 seen = 0;
        for (uint256 i = 0; i < entries.length; ++i) {
            Vm.Log memory logEntry = entries[i];
            if (logEntry.emitter != address(forwarder)) continue;
            if (logEntry.topics.length == 0 || logEntry.topics[0] != _EVENT_APPENDED_SIG) continue;

            ++seen;
            uint256 expectedSeq = seqBefore + seen;
            uint256 gotSeq = uint256(logEntry.topics[1]);
            assertEq(gotSeq, expectedSeq);

            bytes32 prevTip = logEntry.topics[2];
            bytes32 newTip = logEntry.topics[3];
            assertEq(prevTip, tip);

            (bytes32 eventSignature, bytes memory abiEncodedEventData) = abi.decode(logEntry.data, (bytes32, bytes));

            bytes32 computedNewTip = sha256(
                abi.encodePacked(
                    prevTip, expectedSeq, block.number, block.timestamp, eventSignature, abiEncodedEventData
                )
            );
            assertEq(computedNewTip, newTip);
            tip = newTip;
        }

        assertEq(seen, expectedAppends);
        assertEq(forwarder.eventSeq(), seqBefore + expectedAppends);
        assertEq(forwarder.eventChainTip(), tip);
    }
}

contract IntentsForwarderEventChainConstructorTest is Test, EventChainAsserts {
    function test_constructor_appendsOwnershipToChain() external {
        MockERC20 usdt_ = new MockERC20("USDT", "USDT", 6);
        MockERC20 usdc_ = new MockERC20("USDC", "USDC", 6);
        address owner_ = makeAddr("owner");

        vm.recordLogs();
        IntentsForwarder forwarder_ = new IntentsForwarder(address(usdt_), address(usdc_), owner_);
        Vm.Log[] memory entries = vm.getRecordedLogs();

        // Constructor should append exactly one event: OwnershipTransferred(0 -> owner).
        assertEq(forwarder_.eventSeq(), 1);
        assertTrue(forwarder_.eventChainTip() != EventChainGenesis.IntentsForwarderIndex);

        uint256 appendCount = _countEventAppended(entries, address(forwarder_));
        assertEq(appendCount, 1);
    }
}

contract IntentsForwarderEventChainTest is ForwarderTestBase, EventChainAsserts {
    function test_transferOwnership_appendsOwnershipToChain() external {
        uint256 seqBefore = forwarder.eventSeq();
        bytes32 tipBefore = forwarder.eventChainTip();

        address newOwner = makeAddr("newOwner");
        vm.recordLogs();
        vm.prank(owner);
        forwarder.transferOwnership(newOwner);
        Vm.Log[] memory entries = vm.getRecordedLogs();

        _assertAndRecomputeEventChainFromLogs(forwarder, seqBefore, tipBefore, entries, 1);
        assertEq(forwarder.owner(), newOwner);
    }

    function test_eventSeq_delta_local_base_noSwap() external {
        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        address payable baseReceiver = forwarder.predictReceiverAddress(receiverSalt);

        usdt.mint(baseReceiver, 123e6);

        uint256 seqBefore = forwarder.eventSeq();
        bytes32 tipBefore = forwarder.eventChainTip();

        vm.recordLogs();
        forwarder.pullFromReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(1)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
        Vm.Log[] memory entries = vm.getRecordedLogs();

        // Base no-swap path includes: ForwardStarted + ReceiverDeployed(ephemeral) + ReceiverDeployed(base) + ForwardCompleted
        _assertAndRecomputeEventChainFromLogs(forwarder, seqBefore, tipBefore, entries, 4);
    }

    function test_eventSeq_delta_local_base_swap_includesSwapExecuted() external {
        // Configure quoter for swaps from USDT.
        MockQuoter quoter = new MockQuoter();
        quoter.setAmountOut(90e6);
        vm.prank(owner);
        forwarder.setQuoter(address(usdt), quoter);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        address payable baseReceiver = forwarder.predictReceiverAddress(receiverSalt);
        usdt.mint(baseReceiver, 100e6);

        // Swap: mint 100 USDC to the executor so actualOut=100, minOut=90.
        Call[] memory swapData = new Call[](1);
        swapData[0] = Call({
            to: address(usdc), value: 0, data: abi.encodeCall(usdc.mint, (address(forwarder.SWAP_EXECUTOR()), 100e6))
        });

        uint256 seqBefore = forwarder.eventSeq();
        bytes32 tipBefore = forwarder.eventChainTip();

        vm.recordLogs();
        forwarder.pullFromReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(2)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdc),
                swapData: swapData,
                bridgeData: ""
            })
        );
        Vm.Log[] memory entries = vm.getRecordedLogs();

        // Swap path includes: ForwardStarted + ReceiverDeployed(ephemeral) + ReceiverDeployed(base) + SwapExecuted + ForwardCompleted
        _assertAndRecomputeEventChainFromLogs(forwarder, seqBefore, tipBefore, entries, 5);
    }

    function test_revert_doesNotAdvanceEventChain() external {
        // Configure quoter for swaps from USDT.
        MockQuoter quoter = new MockQuoter();
        quoter.setAmountOut(90e6);
        vm.prank(owner);
        forwarder.setQuoter(address(usdt), quoter);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        bytes32 receiverSalt = baseSalt(block.chainid, beneficiary, false, bytes32(0));
        address payable baseReceiver = forwarder.predictReceiverAddress(receiverSalt);
        usdt.mint(baseReceiver, 100e6);

        // Swap: call a reverter, causing SwapExecutor to revert => entire tx reverts.
        MockReverter reverter = new MockReverter();
        Call[] memory swapData = new Call[](1);
        swapData[0] = Call({to: address(reverter), value: 0, data: abi.encodeCall(reverter.boom, ())});

        uint256 seqBefore = forwarder.eventSeq();
        bytes32 tipBefore = forwarder.eventChainTip();

        vm.expectRevert();
        forwarder.pullFromReceiver(
            IntentsForwarder.PullRequest({
                targetChain: block.chainid,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(3)),
                balance: 0,
                tokenIn: address(usdt),
                tokenOut: address(usdc),
                swapData: swapData,
                bridgeData: ""
            })
        );

        assertEq(forwarder.eventSeq(), seqBefore);
        assertEq(forwarder.eventChainTip(), tipBefore);
    }

    function test_eventChainTip_recomputes_fromEventAppendedLogs_bridge() external {
        ExactBridger usdtBridger = new ExactBridger();
        ExactBridger usdcBridger = new ExactBridger();
        vm.prank(owner);
        forwarder.setBridgers(usdtBridger, usdcBridger);

        address payable beneficiary = payable(makeAddr("beneficiary"));
        uint256 targetChain = 999999;

        // Ephemeral mode (balance != 0) so only the ephemeral receiver is deployed.
        bytes32 receiverSalt = baseSalt(targetChain, beneficiary, false, bytes32(0));
        bytes32 ephemSalt = ephemeralSalt(receiverSalt, bytes32(uint256(7)), address(usdt), uint256(5e6));
        address payable receiver = forwarder.predictReceiverAddress(ephemSalt);
        usdt.mint(receiver, 5e6);

        uint256 seqBefore = forwarder.eventSeq();
        bytes32 tipBefore = forwarder.eventChainTip();

        vm.recordLogs();
        forwarder.pullFromReceiver(
            IntentsForwarder.PullRequest({
                targetChain: targetChain,
                beneficiary: beneficiary,
                beneficiaryClaimOnly: false,
                intentHash: bytes32(0),
                forwardSalt: bytes32(uint256(7)),
                balance: 5e6,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: hex"beef"
            })
        );
        Vm.Log[] memory entries = vm.getRecordedLogs();

        // ForwardStarted + ReceiverDeployed(ephemeral) + BridgeInitiated + ForwardCompleted
        _assertAndRecomputeEventChainFromLogs(forwarder, seqBefore, tipBefore, entries, 4);
    }
}
