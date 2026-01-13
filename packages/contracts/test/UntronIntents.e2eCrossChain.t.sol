// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {UntronTestBase} from "./helpers/UntronTestBase.sol";

import {Call} from "../src/SwapExecutor.sol";
import {IntentsForwarder} from "../src/IntentsForwarder.sol";

import {MockTransportBridger} from "../src/mocks/MockTransportBridger.sol";

contract UntronIntentsE2ECrossChainTest is UntronTestBase {
    uint256 internal constant CHAIN_A = 111;
    uint256 internal constant CHAIN_B = 222;

    MockTransportBridger internal transport;

    function setUp() public override {
        super.setUp();
        _setRecommendedFee(10_000, 123);

        transport = new MockTransportBridger();
        vm.prank(owner);
        forwarder.setBridgers(transport, transport);
    }

    function test_E2E_receiverBridge_virtualClaim_proveThenFundAndSettle() public {
        address toTron = makeAddr("toTron");
        bytes32 forwardSalt = keccak256("forwardSalt");
        uint256 amount = 10_000_000;

        // Chain A: user deposits to the ephemeral receiver address that is the bridge destination.
        vm.chainId(CHAIN_A);
        address receiverA =
            _predictEphemeralReceiverFor(CHAIN_B, address(intents), true, toTron, forwardSalt, address(usdt), amount);
        usdt.mint(receiverA, amount);

        // Chain A: relayer pulls from receiver and initiates a "bridge" to chain B.
        IntentsForwarder.PullRequest memory req = IntentsForwarder.PullRequest({
            targetChain: CHAIN_B,
            beneficiary: payable(address(intents)),
            beneficiaryClaimOnly: true,
            intentHash: keccak256(abi.encode(forwarder, toTron)),
            forwardSalt: forwardSalt,
            balance: amount,
            tokenIn: address(usdt),
            tokenOut: address(usdt),
            swapData: new Call[](0),
            bridgeData: ""
        });

        address relayer = makeAddr("relayer");
        vm.prank(relayer);
        forwarder.pullFromReceiver(req);

        // Chain B: solver claims + proves the Tron fill while funds are still "in flight".
        vm.chainId(CHAIN_B);
        bytes32 id = _claimVirtual(solver, toTron, forwardSalt, address(usdt), amount);

        uint256 tronPayAmount = amount - intents.recommendedIntentFee(amount);
        _setTronTx_UsdtTransfer(toTron, tronPayAmount);
        _proveAs(solver, id);

        // Still not funded on chain B.
        assertEq(usdt.balanceOf(solver), 0);

        // Chain B: bridge delivers to the destination receiver address; then funding pulls it into intents and settles.
        transport.deliverLast();
        intents.fundReceiverIntent(forwarder, toTron, forwardSalt, address(usdt), amount);

        assertEq(usdt.balanceOf(solver), intents.INTENT_CLAIM_DEPOSIT() + amount);
    }
}
