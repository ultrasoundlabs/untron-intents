// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {console2} from "forge-std/console2.sol";
import {AutoCreateXScript} from "./utils/AutoCreateXScript.sol";

import {IntentsForwarder} from "../src/IntentsForwarder.sol";
import {Call} from "../src/SwapExecutor.sol";
import {UntronIntents} from "../src/UntronIntents.sol";

import {TriggerSmartContract} from "../src/external/interfaces/ITronTxReader.sol";

import {MockERC20} from "../src/mocks/MockERC20.sol";
import {MockUntronV3} from "../src/mocks/MockUntronV3.sol";
import {MockTronTxReader} from "../src/mocks/MockTronTxReader.sol";
import {MockQuoter} from "../src/mocks/MockQuoter.sol";
import {MockTransportBridger} from "../src/mocks/MockTransportBridger.sol";
import {ExactBridger} from "../src/mocks/MockBridgers.sol";

/// @notice Generates a wide variety of protocol activity for indexer testing.
/// @dev Designed to be run on multiple Anvil instances (different chain IDs) to emulate a multi-chain environment.
///
/// Required env (unless you compute these via CREATE3 externally):
/// - `USDT`, `USDC`, `FORWARDER_ADDRESS`, `INTENTS_ADDRESS`, `UNTRON_V3`
///
/// Optional env:
/// - `MODE`: `"solo" | "spoke" | "hub"` (default `"solo"`)
/// - `HUB_CHAIN_ID`: hub chainId (required for `"spoke"`/`"hub"`)
/// - `SPOKE_CHAIN_IDS`: comma-separated uint256 chainIds (required for `"hub"`)
/// - `TO_TRON`: address used as Tron recipient (default derived)
/// - `FORWARD_SALT`: bytes32 (default `keccak256("forwardSalt")`)
/// - `BASE_AMOUNT`: amount base for receiver flows (default 10_000_000)
/// - `FEE_PPM`, `FEE_FLAT`: UntronIntents recommended fee config (default 10_000, 123)
/// - `DEPLOYER_PK`, `MAKER_PK`, `SOLVER_PK`, `RELAYER_PK`: actor keys (defaults to `DEPLOYER_PK` or Foundry sender)
contract SimulateActivity is AutoCreateXScript {
    function run() public {
        _ensureCreateX();

        // Make time-based flows deterministic on local Anvil chains by ensuring timestamps advance
        // consistently between mined blocks.
        if (_isAnvilRpc()) {
            _rpc("anvil_setBlockTimestampInterval", "[1]");
        }

        string memory mode = vm.envOr("MODE", string("solo"));

        uint256 deployerPk = vm.envOr("DEPLOYER_PK", uint256(0));
        uint256 makerPk = vm.envOr("MAKER_PK", deployerPk);
        uint256 solverPk = vm.envOr("SOLVER_PK", deployerPk);
        uint256 relayerPk = vm.envOr("RELAYER_PK", deployerPk);

        // Either pass addresses explicitly, or let the script predict CREATE3 addresses from the
        // same salt scheme as `script/DeployProtocol.s.sol`.
        bool useCreate3Prediction = vm.envOr("USE_CREATE3_PREDICTION", true);

        address usdtAddr = vm.envOr("USDT", address(0));
        address usdcAddr = vm.envOr("USDC", address(0));
        address forwarderAddr = vm.envOr("FORWARDER_ADDRESS", address(0));
        address intentsAddr = vm.envOr("INTENTS_ADDRESS", address(0));
        address v3Addr = vm.envOr("UNTRON_V3", address(0));

        if (
            useCreate3Prediction
                && (usdtAddr == address(0)
                    || usdcAddr == address(0)
                    || forwarderAddr == address(0)
                    || intentsAddr == address(0)
                    || v3Addr == address(0))
        ) {
            address deployer = _predictDeployer(deployerPk);

            uint8 crossChainFlag = uint8(vm.envOr("CREATE3_CROSS_CHAIN_FLAG", uint256(0)));
            uint88 saltIdForwarder = uint88(vm.envOr("SALT_ID_FORWARDER", uint256(1)));
            uint88 saltIdIntents = uint88(vm.envOr("SALT_ID_INTENTS", uint256(2)));
            uint88 saltIdMockUsdt = uint88(vm.envOr("SALT_ID_MOCK_USDT", uint256(100)));
            uint88 saltIdMockUsdc = uint88(vm.envOr("SALT_ID_MOCK_USDC", uint256(101)));
            uint88 saltIdMockV3 = uint88(vm.envOr("SALT_ID_MOCK_V3", uint256(111)));

            if (usdtAddr == address(0)) {
                usdtAddr = computeCreate3Address(_salt(deployer, crossChainFlag, saltIdMockUsdt), deployer);
            }
            if (usdcAddr == address(0)) {
                usdcAddr = computeCreate3Address(_salt(deployer, crossChainFlag, saltIdMockUsdc), deployer);
            }
            if (forwarderAddr == address(0)) {
                forwarderAddr = computeCreate3Address(_salt(deployer, crossChainFlag, saltIdForwarder), deployer);
            }
            if (intentsAddr == address(0)) {
                intentsAddr = computeCreate3Address(_salt(deployer, crossChainFlag, saltIdIntents), deployer);
            }
            if (v3Addr == address(0)) {
                v3Addr = computeCreate3Address(_salt(deployer, crossChainFlag, saltIdMockV3), deployer);
            }
        }

        require(usdtAddr != address(0) && usdcAddr != address(0), "missing USDT/USDC");
        require(forwarderAddr != address(0) && intentsAddr != address(0), "missing FORWARDER_ADDRESS/INTENTS_ADDRESS");
        require(v3Addr != address(0), "missing UNTRON_V3");

        require(usdtAddr.code.length != 0 && usdcAddr.code.length != 0, "token not deployed");
        require(forwarderAddr.code.length != 0 && intentsAddr.code.length != 0, "core not deployed");
        require(v3Addr.code.length != 0, "v3 not deployed");

        MockERC20 usdt = MockERC20(usdtAddr);
        MockERC20 usdc = MockERC20(usdcAddr);
        IntentsForwarder forwarder = IntentsForwarder(payable(forwarderAddr));
        UntronIntents intents = UntronIntents(intentsAddr);

        uint256 feePpm = vm.envOr("FEE_PPM", uint256(10_000));
        uint256 feeFlat = vm.envOr("FEE_FLAT", uint256(123));
        uint256 baseAmount = vm.envOr("BASE_AMOUNT", uint256(10_000_000));

        bytes32 forwardSalt = vm.envOr("FORWARD_SALT", keccak256("forwardSalt"));
        address toTron = vm.envOr("TO_TRON", address(uint160(uint256(keccak256("toTron")))));

        console2.log("chainId", block.chainid);
        console2.log("mode", mode);
        console2.log("forwarder", address(forwarder));
        console2.log("intents", address(intents));
        console2.log("usdt", address(usdt));
        console2.log("usdc", address(usdc));
        console2.log("toTron", toTron);

        _startBroadcast(deployerPk);
        _maybeConfigureOwners(forwarder, intents, feePpm, feeFlat);
        _stopBroadcast();

        // Local activity on every chain.
        _localForwarderActivity(usdt, usdc, forwarder, deployerPk, relayerPk);
        _localIntentsActivity(usdt, usdc, forwarder, intents, v3Addr, makerPk, solverPk, deployerPk);

        if (_eq(mode, "spoke")) {
            uint256 hubChainId = vm.envOr("HUB_CHAIN_ID", uint256(0));
            require(hubChainId != 0, "missing HUB_CHAIN_ID");
            _spokeBridgeToHub(usdt, forwarder, intents, toTron, forwardSalt, baseAmount, relayerPk, hubChainId);
        } else if (_eq(mode, "hub")) {
            uint256 hubChainId = vm.envOr("HUB_CHAIN_ID", uint256(0));
            require(hubChainId != 0, "missing HUB_CHAIN_ID");
            require(hubChainId == block.chainid, "hub mode must run on HUB_CHAIN_ID");

            uint256[] memory spokeChainIds = vm.envOr("SPOKE_CHAIN_IDS", ",", new uint256[](0));
            require(spokeChainIds.length != 0, "missing SPOKE_CHAIN_IDS");

            _hubProcessInbound(
                usdt, forwarder, intents, v3Addr, toTron, forwardSalt, baseAmount, solverPk, spokeChainIds
            );
        }
    }

    /*//////////////////////////////////////////////////////////////
                               LOCAL ACTIVITY
    //////////////////////////////////////////////////////////////*/

    function _localForwarderActivity(
        MockERC20 usdt,
        MockERC20 usdc,
        IntentsForwarder forwarder,
        uint256 deployerPk,
        uint256 relayerPk
    ) internal {
        // Configure quoter + bridgers to deterministic mocks (if caller is owner).
        _startBroadcast(deployerPk);
        if (forwarder.owner() == msg.sender) {
            // Quoter: deterministic minOut.
            MockQuoter q = new MockQuoter();
            q.setAmountOut(90e6);
            forwarder.setQuoter(address(usdt), q);

            // Bridgers: record-only mock.
            MockTransportBridger transport = new MockTransportBridger();
            forwarder.setBridgers(transport, transport);
        }
        _stopBroadcast();

        address payable beneficiary = payable(address(uint160(uint256(keccak256("beneficiary")))));

        // Base-mode local pull (balance=0).
        {
            bytes32 receiverSalt = keccak256(abi.encodePacked(block.chainid, beneficiary, false, bytes32(0)));
            address receiver = forwarder.predictReceiverAddress(receiverSalt);
            _mint(usdt, deployerPk, receiver, 123e6);

            _startBroadcast(relayerPk);
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
            _stopBroadcast();
        }

        // Base-mode swap (USDT -> USDC) with relayer rebate.
        {
            bytes32 receiverSalt = keccak256(abi.encodePacked(block.chainid, beneficiary, false, bytes32(0)));
            address receiver = forwarder.predictReceiverAddress(receiverSalt);
            _mint(usdt, deployerPk, receiver, 100e6);

            Call[] memory swapData = new Call[](1);
            swapData[0] = Call({
                to: address(usdc),
                value: 0,
                data: abi.encodeCall(usdc.mint, (address(forwarder.SWAP_EXECUTOR()), 100e6))
            });

            _startBroadcast(relayerPk);
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
            _stopBroadcast();
        }
    }

    function _localIntentsActivity(
        MockERC20 usdt,
        MockERC20 usdc,
        IntentsForwarder forwarder,
        UntronIntents intents,
        address v3Addr,
        uint256 makerPk,
        uint256 solverPk,
        uint256 deployerPk
    ) internal {
        bytes32 closeIntentId;
        bytes32 unclaimIntentId;

        // Maker-funded TriggerSmartContract intent (create -> claim -> prove -> settle).
        {
            uint256 escrow = 5_000_000;
            _startBroadcast(makerPk);
            address maker = msg.sender;
            usdc.mint(maker, escrow);
            usdc.approve(address(intents), type(uint256).max);

            UntronIntents.Intent memory intent = UntronIntents.Intent({
                intentType: UntronIntents.IntentType.TRIGGER_SMART_CONTRACT,
                intentSpecs: abi.encode(
                    UntronIntents.TriggerSmartContractIntent({
                        to: address(uint160(uint256(keccak256("tronTarget")))), data: hex"abcd"
                    })
                ),
                refundBeneficiary: maker,
                token: address(usdc),
                amount: escrow
            });

            uint256 deadline = block.timestamp + 60;
            intents.createIntent(intent, deadline);
            _stopBroadcast();

            bytes32 intentHash = keccak256(abi.encode(intent));
            bytes32 id = keccak256(abi.encodePacked(maker, intentHash, deadline));

            _mintApproveDeposit(usdt, solverPk, address(intents), intents.INTENT_CLAIM_DEPOSIT());
            _startBroadcast(solverPk);
            intents.claimIntent(id);
            _stopBroadcast();

            // Mock a Tron tx that matches the intent.
            _setTronTx_TriggerMatch(deployerPk, v3Addr, intent.intentSpecs);

            _startBroadcast(solverPk);
            intents.proveIntentFill(id, _emptyBlocks(), "", _emptyProof(), 0);
            _stopBroadcast();
        }

        // Maker-funded USDT transfer intent (create -> claim -> prove -> settle).
        {
            uint256 escrow = 2_000_000;
            _startBroadcast(makerPk);
            address maker = msg.sender;
            usdt.mint(maker, escrow);
            usdt.approve(address(intents), type(uint256).max);
            address toTron = address(uint160(uint256(keccak256("toTronLocal"))));

            UntronIntents.Intent memory intent = UntronIntents.Intent({
                intentType: UntronIntents.IntentType.USDT_TRANSFER,
                intentSpecs: abi.encode(UntronIntents.USDTTransferIntent({to: toTron, amount: 1_234_567})),
                refundBeneficiary: maker,
                token: address(usdt),
                amount: escrow
            });

            uint256 deadline = block.timestamp + 60;
            intents.createIntent(intent, deadline);
            _stopBroadcast();

            bytes32 intentHash = keccak256(abi.encode(intent));
            bytes32 id = keccak256(abi.encodePacked(maker, intentHash, deadline));

            _mintApproveDeposit(usdt, solverPk, address(intents), intents.INTENT_CLAIM_DEPOSIT());
            _startBroadcast(solverPk);
            intents.claimIntent(id);
            _stopBroadcast();

            _setTronTx_UsdtTransferMatch(deployerPk, v3Addr, toTron, 1_234_567);

            _startBroadcast(solverPk);
            intents.proveIntentFill(id, _emptyBlocks(), "", _emptyProof(), 0);
            _stopBroadcast();
        }

        // Close: create an unsolved intent we will close later (after time has advanced).
        // The follow-up close is performed by `script/SimulateTimedActions.s.sol`.
        {
            uint256 escrow = 1_000_000;
            _startBroadcast(makerPk);
            address maker = msg.sender;
            usdc.mint(maker, escrow);
            usdc.approve(address(intents), type(uint256).max);

            UntronIntents.Intent memory intent = UntronIntents.Intent({
                intentType: UntronIntents.IntentType.TRIGGER_SMART_CONTRACT,
                intentSpecs: abi.encode(
                    UntronIntents.TriggerSmartContractIntent({
                        to: address(uint160(uint256(keccak256("tronTarget2")))), data: hex"beef"
                    })
                ),
                refundBeneficiary: maker,
                token: address(usdc),
                amount: escrow
            });

            uint256 deadline = block.timestamp + 60;
            intents.createIntent(intent, deadline);
            _stopBroadcast();

            bytes32 intentHash = keccak256(abi.encode(intent));
            closeIntentId = keccak256(abi.encodePacked(maker, intentHash, deadline));
        }

        // Unclaim: create a time-gated claim we will clear later (after TIME_TO_FILL).
        // The follow-up unclaim is performed by `script/SimulateTimedActions.s.sol`.
        {
            address toTron = address(uint160(uint256(keccak256("toTronVirtualUnfunded"))));
            bytes32 forwardSalt = keccak256("virtual-unfunded");
            uint256 amount = 1_000_000;

            _mintApproveDeposit(usdt, solverPk, address(intents), intents.INTENT_CLAIM_DEPOSIT());

            _startBroadcast(solverPk);
            intents.claimVirtualReceiverIntent(forwarder, toTron, forwardSalt, address(usdt), amount);
            _stopBroadcast();

            unclaimIntentId = intents.receiverIntentId(forwarder, toTron, forwardSalt, address(usdt), amount);
        }

        // Persist ids needed for time-gated follow-up actions (close/unclaim) so the runner can
        // advance time via RPC and execute them in a subsequent script.
        _writeTimedState(forwarder, intents, closeIntentId, unclaimIntentId);
    }

    /*//////////////////////////////////////////////////////////////
                           CROSS-CHAIN SIMULATION
    //////////////////////////////////////////////////////////////*/

    function _spokeBridgeToHub(
        MockERC20 usdt,
        IntentsForwarder forwarder,
        UntronIntents intents,
        address toTron,
        bytes32 forwardSalt,
        uint256 baseAmount,
        uint256 relayerPk,
        uint256 hubChainId
    ) internal {
        // Mint USDT into the *ephemeral receiver address* that is the bridge destination on the hub.
        // On the spoke, that same address is used as the pull source in ephemeral mode.
        bytes32 intentHash = keccak256(abi.encode(forwarder, toTron));
        bytes32 baseReceiverSalt = keccak256(abi.encodePacked(hubChainId, address(intents), true, intentHash));

        uint256 amount = baseAmount + (block.chainid % 10_000);
        bytes32 ephemSalt = keccak256(abi.encodePacked(baseReceiverSalt, forwardSalt, address(usdt), amount));
        address receiver = forwarder.predictReceiverAddress(ephemSalt);

        _mint(usdt, relayerPk, receiver, amount);

        _startBroadcast(relayerPk);
        forwarder.pullFromReceiver(
            IntentsForwarder.PullRequest({
                targetChain: hubChainId,
                beneficiary: payable(address(intents)),
                beneficiaryClaimOnly: true,
                intentHash: intentHash,
                forwardSalt: forwardSalt,
                balance: amount,
                tokenIn: address(usdt),
                tokenOut: address(usdt),
                swapData: new Call[](0),
                bridgeData: ""
            })
        );
        _stopBroadcast();
    }

    function _hubProcessInbound(
        MockERC20 usdt,
        IntentsForwarder forwarder,
        UntronIntents intents,
        address v3Addr,
        address toTron,
        bytes32 forwardSalt,
        uint256 baseAmount,
        uint256 solverPk,
        uint256[] memory spokeChainIds
    ) internal {
        _mintApproveDeposit(usdt, solverPk, address(intents), intents.INTENT_CLAIM_DEPOSIT() * spokeChainIds.length);

        address payable beneficiary = payable(address(uint160(uint256(keccak256("hubBeneficiary")))));

        for (uint256 i = 0; i < spokeChainIds.length; ++i) {
            uint256 spokeChainId = spokeChainIds[i];
            if (spokeChainId == block.chainid) continue;

            // This must match the spoke amount formula.
            uint256 amount = baseAmount + (spokeChainId % 10_000);

            bytes32 intentHash = keccak256(abi.encode(forwarder, toTron));
            bytes32 baseReceiverSalt = keccak256(abi.encodePacked(block.chainid, address(intents), true, intentHash));
            bytes32 ephemSalt = keccak256(abi.encodePacked(baseReceiverSalt, forwardSalt, address(usdt), amount));
            address receiver = forwarder.predictReceiverAddress(ephemSalt);

            // Simulate bridge delivery by minting to the counterfactual receiver address.
            _mint(usdt, solverPk, receiver, amount);

            // Virtual claim (solver) + prove, then fund (pull from receiver) and settle.
            _startBroadcast(solverPk);
            intents.claimVirtualReceiverIntent(forwarder, toTron, forwardSalt, address(usdt), amount);
            _stopBroadcast();

            bytes32 id = intents.receiverIntentId(forwarder, toTron, forwardSalt, address(usdt), amount);

            uint256 tronPayment = amount - intents.recommendedIntentFee(amount);
            _setTronTx_UsdtTransferMatch(solverPk, v3Addr, toTron, tronPayment);

            _startBroadcast(solverPk);
            intents.proveIntentFill(id, _emptyBlocks(), "", _emptyProof(), 0);
            _stopBroadcast();

            _startBroadcast(solverPk);
            intents.fundReceiverIntent(forwarder, toTron, forwardSalt, address(usdt), amount);
            _stopBroadcast();

            // Emit a small additional forwarder event on hub: local payout from a base receiver.
            bytes32 localBaseSalt = keccak256(abi.encodePacked(block.chainid, beneficiary, false, bytes32(id)));
            address localReceiver = forwarder.predictReceiverAddress(localBaseSalt);
            _mint(usdt, solverPk, localReceiver, 1e6);
            _startBroadcast(solverPk);
            forwarder.pullFromReceiver(
                IntentsForwarder.PullRequest({
                    targetChain: block.chainid,
                    beneficiary: beneficiary,
                    beneficiaryClaimOnly: false,
                    intentHash: bytes32(id),
                    forwardSalt: bytes32(uint256(99)),
                    balance: 0,
                    tokenIn: address(usdt),
                    tokenOut: address(usdt),
                    swapData: new Call[](0),
                    bridgeData: ""
                })
            );
            _stopBroadcast();
        }
    }

    /*//////////////////////////////////////////////////////////////
                                 HELPERS
    //////////////////////////////////////////////////////////////*/

    function _maybeConfigureOwners(IntentsForwarder forwarder, UntronIntents intents, uint256 feePpm, uint256 feeFlat)
        internal
    {
        // Best-effort config: if caller is owner, set bridgers/quoter/fees to deterministic values.
        if (forwarder.owner() == msg.sender) {
            ExactBridger bridger = new ExactBridger();
            forwarder.setBridgers(bridger, bridger);

            MockQuoter q = new MockQuoter();
            q.setAmountOut(90e6);
            forwarder.setQuoter(forwarder.USDT(), q);
        }
        if (intents.owner() == msg.sender) {
            intents.setRecommendedIntentFee(feePpm, feeFlat);
        }
    }

    function _mintApproveDeposit(MockERC20 usdt, uint256 pk, address spender, uint256 amount) internal {
        _startBroadcast(pk);
        usdt.mint(msg.sender, amount);
        usdt.approve(spender, type(uint256).max);
        _stopBroadcast();
    }

    function _setTronTx_TriggerMatch(uint256 pk, address v3Addr, bytes memory intentSpecs) internal {
        // Requires MockUntronV3 + MockTronTxReader.
        MockUntronV3 v3 = MockUntronV3(v3Addr);
        MockTronTxReader reader = MockTronTxReader(address(v3.tronReader()));

        UntronIntents.TriggerSmartContractIntent memory specs =
            abi.decode(intentSpecs, (UntronIntents.TriggerSmartContractIntent));

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(specs.to);
        tx_.data = specs.data;
        _startBroadcast(pk);
        reader.setTx(tx_);
        _stopBroadcast();
    }

    function _setTronTx_UsdtTransferMatch(uint256 pk, address v3Addr, address toTron, uint256 amount) internal {
        MockUntronV3 v3 = MockUntronV3(v3Addr);
        MockTronTxReader reader = MockTronTxReader(address(v3.tronReader()));

        TriggerSmartContract memory tx_;
        tx_.toTron = _tronAddrBytes21(v3.tronUsdt());
        tx_.data = abi.encodeWithSelector(bytes4(keccak256("transfer(address,uint256)")), toTron, amount);
        _startBroadcast(pk);
        reader.setTx(tx_);
        _stopBroadcast();
    }

    function _tronAddrBytes21(address tronAddr) internal pure returns (bytes21 out) {
        uint168 packed = (uint168(uint8(0x41)) << 160) | uint168(uint160(tronAddr));
        return bytes21(packed);
    }

    function _startBroadcast(uint256 pk) internal {
        if (pk != 0) vm.startBroadcast(pk);
        else vm.startBroadcast();
    }

    function _stopBroadcast() internal {
        vm.stopBroadcast();
    }

    function _writeTimedState(
        IntentsForwarder forwarder,
        UntronIntents intents,
        bytes32 closeIntentId,
        bytes32 unclaimIntentId
    ) internal {
        string memory obj = "activity";
        string memory json = vm.serializeUint(obj, "chainId", block.chainid);
        json = vm.serializeAddress(obj, "forwarder", address(forwarder));
        json = vm.serializeAddress(obj, "intents", address(intents));
        json = vm.serializeBytes32(obj, "closeIntentId", closeIntentId);
        json = vm.serializeBytes32(obj, "unclaimIntentId", unclaimIntentId);

        string memory path = string.concat("out/activity-state-", vm.toString(block.chainid), ".json");
        vm.writeJson(json, path);
        console2.log("wrote activity state", path);
    }

    function _mint(MockERC20 token, uint256 pk, address to, uint256 amount) internal {
        _startBroadcast(pk);
        token.mint(to, amount);
        _stopBroadcast();
    }

    function _salt(address deployer, uint8 crossChainFlag, uint88 saltId) internal pure returns (bytes32) {
        return bytes32(abi.encodePacked(deployer, bytes1(crossChainFlag), bytes11(saltId)));
    }

    function _predictDeployer(uint256 pk) internal returns (address deployer) {
        _startBroadcast(pk);
        deployer = msg.sender;
        _stopBroadcast();
    }

    function _emptyBlocks() internal pure returns (bytes[20] memory blocks) {
        return blocks;
    }

    function _emptyProof() internal pure returns (bytes32[] memory proof) {
        return proof;
    }

    function _eq(string memory a, string memory b) internal pure returns (bool) {
        return keccak256(bytes(a)) == keccak256(bytes(b));
    }
}
