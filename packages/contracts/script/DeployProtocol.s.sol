// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {console2} from "forge-std/console2.sol";

import {AutoCreateXScript} from "./utils/AutoCreateXScript.sol";

import {IntentsForwarder} from "../src/IntentsForwarder.sol";
import {UntronIntents} from "../src/UntronIntents.sol";

import {IBridger} from "../src/bridgers/interfaces/IBridger.sol";
import {USDT0Bridger} from "../src/bridgers/USDT0Bridger.sol";
import {CCTPV2Bridger} from "../src/bridgers/CCTPV2Bridger.sol";

import {StablecoinQuoter} from "../src/quoters/StablecoinQuoter.sol";
import {IQuoter} from "../src/quoters/interfaces/IQuoter.sol";

import {MockERC20} from "../src/mocks/MockERC20.sol";
import {MockQuoter} from "../src/mocks/MockQuoter.sol";
import {ExactBridger, FeeBridger, RevertingBridger} from "../src/mocks/MockBridgers.sol";
import {MockTransportBridger} from "../src/mocks/MockTransportBridger.sol";
import {MockForwarderPuller} from "../src/mocks/MockForwarderPuller.sol";
import {MockTronTxReader} from "../src/mocks/MockTronTxReader.sol";
import {MockUntronV3} from "../src/mocks/MockUntronV3.sol";

/// @notice Modular deployment script for the Untron Intents protocol (prod or mock deployments).
/// @dev Uses CreateX (CREATE3) for deterministic deployments.
///
/// Quickstart examples (simulation):
/// - `forge script script/DeployProtocol.s.sol:DeployProtocol`
///
/// Deploy all-mocks to Anvil:
/// - Ensure CreateX exists on the chain (see `lib/createx-forge/README.md` about Anvil chain IDs).
/// - `MOCKS=true DEPLOY_ALL=true forge script script/DeployProtocol.s.sol:DeployProtocol --rpc-url $RPC --broadcast --private-key $PK`
contract DeployProtocol is AutoCreateXScript {
    /*//////////////////////////////////////////////////////////////
                                 TYPES
    //////////////////////////////////////////////////////////////*/

    struct Addrs {
        address usdt;
        address usdc;

        address forwarder;
        address intents;

        address v3;
        address tronReader;

        address usdtBridger;
        address usdcBridger;

        address quoter;
    }

    /*//////////////////////////////////////////////////////////////
                            SCRIPT SETUP
    //////////////////////////////////////////////////////////////*/

    /*//////////////////////////////////////////////////////////////
                                  RUN
    //////////////////////////////////////////////////////////////*/

    function run() public {
        _ensureCreateX();

        bool deployAll = vm.envOr("DEPLOY_ALL", false);
        bool mocks = vm.envOr("MOCKS", false);

        bool deployForwarder = vm.envOr("DEPLOY_FORWARDER", deployAll);
        bool deployIntents = vm.envOr("DEPLOY_INTENTS", deployAll);

        bool deployMockTokens = vm.envOr("DEPLOY_MOCK_TOKENS", mocks);
        bool deployMockV3 = vm.envOr("DEPLOY_MOCK_V3", mocks);

        bool deployUsdtBridger = vm.envOr("DEPLOY_USDT_BRIDGER", deployAll || mocks);
        bool deployUsdcBridger = vm.envOr("DEPLOY_USDC_BRIDGER", deployAll || mocks);
        bool deployQuoter = vm.envOr("DEPLOY_QUOTER", deployAll || mocks);

        bool configureForwarder = vm.envOr("CONFIGURE_FORWARDER", deployAll || mocks);
        bool configureIntents = vm.envOr("CONFIGURE_INTENTS", deployAll || mocks);

        // If provided, these override CREATE3-computed addresses.
        address forwarderOverride = vm.envOr("FORWARDER_ADDRESS", address(0));
        address intentsOverride = vm.envOr("INTENTS_ADDRESS", address(0));

        // `real` (IntentsForwarder) or `mock_puller` (MockForwarderPuller).
        string memory forwarderKind = vm.envOr("FORWARDER_KIND", string("real"));

        // Optional: if provided, these are used unless `DEPLOY_MOCK_TOKENS=true`.
        address usdtOverride = vm.envOr("USDT", address(0));
        address usdcOverride = vm.envOr("USDC", address(0));

        // Optional: if provided, used unless `DEPLOY_MOCK_V3=true`.
        address v3Override = vm.envOr("UNTRON_V3", address(0));

        // Salts: CreateX recommends prefixing with deployer, plus 1-byte cross-chain flag, plus 11 bytes of salt-id.
        uint8 crossChainFlag = uint8(vm.envOr("CREATE3_CROSS_CHAIN_FLAG", uint256(0)));
        uint88 saltIdForwarder = uint88(vm.envOr("SALT_ID_FORWARDER", uint256(1)));
        uint88 saltIdIntents = uint88(vm.envOr("SALT_ID_INTENTS", uint256(2)));
        uint88 saltIdMockUsdt = uint88(vm.envOr("SALT_ID_MOCK_USDT", uint256(100)));
        uint88 saltIdMockUsdc = uint88(vm.envOr("SALT_ID_MOCK_USDC", uint256(101)));
        uint88 saltIdMockTronReader = uint88(vm.envOr("SALT_ID_MOCK_TRON_READER", uint256(110)));
        uint88 saltIdMockV3 = uint88(vm.envOr("SALT_ID_MOCK_V3", uint256(111)));
        uint88 saltIdUsdtBridger = uint88(vm.envOr("SALT_ID_USDT_BRIDGER", uint256(200)));
        uint88 saltIdUsdcBridger = uint88(vm.envOr("SALT_ID_USDC_BRIDGER", uint256(201)));
        uint88 saltIdQuoter = uint88(vm.envOr("SALT_ID_QUOTER", uint256(300)));

        // Configuration: ownership / admin.
        address forwarderOwner = vm.envOr("FORWARDER_OWNER", address(0));
        address forwarderFinalOwner = vm.envOr("FORWARDER_FINAL_OWNER", address(0));
        address intentsOwner = vm.envOr("INTENTS_OWNER", address(0));
        address intentsFinalOwner = vm.envOr("INTENTS_FINAL_OWNER", address(0));

        // Intents recommended fee config.
        uint256 feePpm = vm.envOr("INTENTS_FEE_PPM", uint256(0));
        uint256 feeFlat = vm.envOr("INTENTS_FEE_FLAT", uint256(0));

        // Bridger/quoter selection:
        // - `existing`: use *_ADDRESS env var
        // - `mock_exact`: deploy ExactBridger (EVM stable-in == stable-out, expectedOut == inputAmount)
        // - `mock_fee`: deploy FeeBridger (expectedOut == inputAmount - fee; sets fee from env)
        // - `mock_revert`: deploy RevertingBridger
        // - `mock_transport`: deploy MockTransportBridger (manual delivery)
        // - `usdt0`: deploy USDT0Bridger
        // - `cctp`: deploy CCTPV2Bridger
        string memory usdtBridgerKind =
            vm.envOr("USDT_BRIDGER_KIND", mocks ? "mock_transport" : (deployAll ? "usdt0" : "existing"));
        string memory usdcBridgerKind =
            vm.envOr("USDC_BRIDGER_KIND", mocks ? "mock_transport" : (deployAll ? "cctp" : "existing"));
        string memory quoterKind = vm.envOr("QUOTER_KIND", mocks ? "mock" : (deployAll ? "stablecoin" : "existing"));

        // For mock_fee bridger.
        uint256 feeBridgerFee = vm.envOr("FEE_BRIDGER_FEE", uint256(0));

        // For real bridgers.
        address bridgerOwner = vm.envOr("BRIDGER_OWNER", address(0));
        address usdt0Token = vm.envOr("USDT0_TOKEN", address(0));
        address usdt0Oft = vm.envOr("USDT0_OFT", address(0));
        uint256[] memory usdt0SupportedChainIds = vm.envOr("USDT0_SUPPORTED_CHAIN_IDS", ",", new uint256[](0));
        uint256[] memory usdt0EidsU256 = vm.envOr("USDT0_EIDS", ",", new uint256[](0));

        address cctpTokenMessengerV2 = vm.envOr("CCTP_TOKEN_MESSENGER_V2", address(0));
        uint256[] memory cctpSupportedChainIds = vm.envOr("CCTP_SUPPORTED_CHAIN_IDS", ",", new uint256[](0));
        uint256[] memory cctpDomainsU256 = vm.envOr("CCTP_DOMAINS", ",", new uint256[](0));

        // Quoter configuration (tokenIn -> quoter).
        address[] memory quoterTokenIns = vm.envOr("QUOTER_TOKEN_INS", ",", new address[](0));
        address[] memory quoterAddrs = vm.envOr("QUOTER_ADDRS", ",", new address[](0));

        // Optional mock V3 config.
        address tronController = vm.envOr("TRON_CONTROLLER", address(uint160(uint256(keccak256("tronController")))));
        address tronUsdt = vm.envOr("TRON_USDT", address(uint160(uint256(keccak256("tronUsdt")))));

        // Start broadcasting (if `--broadcast` is passed, this is a real broadcast).
        uint256 pk = vm.envOr("PRIVATE_KEY", uint256(0));
        if (pk != 0) vm.startBroadcast(pk);
        else vm.startBroadcast();

        address deployer = msg.sender;
        console2.log("chainId", block.chainid);
        console2.log("deployer", deployer);
        console2.log("deployAll", deployAll);
        console2.log("mocks", mocks);

        if (forwarderOwner == address(0)) forwarderOwner = deployer;
        if (intentsOwner == address(0)) intentsOwner = deployer;
        if (bridgerOwner == address(0)) bridgerOwner = deployer;

        if (forwarderFinalOwner == address(0)) forwarderFinalOwner = forwarderOwner;
        if (intentsFinalOwner == address(0)) intentsFinalOwner = intentsOwner;

        Addrs memory a;

        // Tokens
        if (!deployMockTokens) {
            a.usdt = usdtOverride;
            a.usdc = usdcOverride;
        }
        if (a.usdt == address(0) && deployMockTokens) {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdMockUsdt);
            a.usdt = _deployCreate3IfMissing(
                "MockUSDT", salt, abi.encodePacked(type(MockERC20).creationCode, abi.encode("USDT", "USDT", uint8(6)))
            );
        }
        if (a.usdc == address(0) && deployMockTokens) {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdMockUsdc);
            a.usdc = _deployCreate3IfMissing(
                "MockUSDC", salt, abi.encodePacked(type(MockERC20).creationCode, abi.encode("USDC", "USDC", uint8(6)))
            );
        }
        require(a.usdt != address(0), "USDT not set (provide USDT or DEPLOY_MOCK_TOKENS=true)");
        require(a.usdc != address(0), "USDC not set (provide USDC or DEPLOY_MOCK_TOKENS=true)");

        // Untron V3 config (real or mock)
        if (!deployMockV3) {
            a.v3 = v3Override;
        } else {
            bytes32 readerSalt = _salt(deployer, crossChainFlag, saltIdMockTronReader);
            a.tronReader = _deployCreate3IfMissing("MockTronTxReader", readerSalt, type(MockTronTxReader).creationCode);

            bytes32 v3Salt = _salt(deployer, crossChainFlag, saltIdMockV3);
            a.v3 = _deployCreate3IfMissing(
                "MockUntronV3",
                v3Salt,
                abi.encodePacked(type(MockUntronV3).creationCode, abi.encode(a.tronReader, tronController, tronUsdt))
            );
        }
        require(a.v3 != address(0), "UNTRON_V3 not set (provide UNTRON_V3 or DEPLOY_MOCK_V3=true)");

        // Forwarder (IntentsForwarder)
        if (forwarderOverride != address(0)) {
            a.forwarder = forwarderOverride;
        } else {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdForwarder);
            if (deployForwarder) {
                if (_eq(forwarderKind, "real")) {
                    a.forwarder = _deployCreate3IfMissing(
                        "IntentsForwarder",
                        salt,
                        abi.encodePacked(
                            type(IntentsForwarder).creationCode, abi.encode(a.usdt, a.usdc, forwarderOwner)
                        )
                    );
                } else if (_eq(forwarderKind, "mock_puller")) {
                    a.forwarder =
                        _deployCreate3IfMissing("MockForwarderPuller", salt, type(MockForwarderPuller).creationCode);
                } else {
                    revert("Invalid FORWARDER_KIND");
                }
            } else {
                a.forwarder = computeCreate3Address(salt, deployer);
                console2.log("predicted forwarder", a.forwarder);
            }
        }

        // Intents (UntronIntents)
        if (intentsOverride != address(0)) {
            a.intents = intentsOverride;
        } else {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdIntents);
            if (deployIntents) {
                a.intents = _deployCreate3IfMissing(
                    "UntronIntents",
                    salt,
                    abi.encodePacked(type(UntronIntents).creationCode, abi.encode(intentsOwner, a.v3, a.usdt))
                );
            } else {
                a.intents = computeCreate3Address(salt, deployer);
                console2.log("predicted intents", a.intents);
            }
        }

        // Quoter
        if (deployQuoter) {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdQuoter);
            if (_eq(quoterKind, "stablecoin")) {
                a.quoter = _deployCreate3IfMissing("StablecoinQuoter", salt, type(StablecoinQuoter).creationCode);
            } else if (_eq(quoterKind, "mock")) {
                a.quoter = _deployCreate3IfMissing("MockQuoter", salt, type(MockQuoter).creationCode);
            } else if (_eq(quoterKind, "existing")) {
                a.quoter = vm.envOr("QUOTER_ADDRESS", address(0));
            } else {
                revert("Invalid QUOTER_KIND");
            }
        } else {
            a.quoter = vm.envOr("QUOTER_ADDRESS", address(0));
        }

        // Bridgers
        if (deployUsdtBridger) {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdUsdtBridger);
            a.usdtBridger = _deployBridger(
                usdtBridgerKind,
                salt,
                bridgerOwner,
                a.forwarder,
                feeBridgerFee,
                usdt0Token,
                usdt0Oft,
                usdt0SupportedChainIds,
                usdt0EidsU256,
                cctpTokenMessengerV2,
                a.usdc,
                cctpSupportedChainIds,
                cctpDomainsU256
            );
        } else {
            a.usdtBridger = vm.envOr("USDT_BRIDGER_ADDRESS", address(0));
        }

        if (deployUsdcBridger) {
            bytes32 salt = _salt(deployer, crossChainFlag, saltIdUsdcBridger);
            a.usdcBridger = _deployBridger(
                usdcBridgerKind,
                salt,
                bridgerOwner,
                a.forwarder,
                feeBridgerFee,
                usdt0Token,
                usdt0Oft,
                usdt0SupportedChainIds,
                usdt0EidsU256,
                cctpTokenMessengerV2,
                a.usdc,
                cctpSupportedChainIds,
                cctpDomainsU256
            );
        } else {
            a.usdcBridger = vm.envOr("USDC_BRIDGER_ADDRESS", address(0));
        }

        // Wire: forwarder configuration (bridgers + quoter mapping)
        if (configureForwarder && _eq(forwarderKind, "real")) {
            require(a.forwarder.code.length != 0, "forwarder not deployed");
            IntentsForwarder f = IntentsForwarder(payable(a.forwarder));
            require(
                f.owner() == deployer,
                "not forwarder owner (deploy with FORWARDER_OWNER=deployer or broadcast as owner)"
            );

            if (a.usdtBridger != address(0) && a.usdcBridger != address(0)) {
                f.setBridgers(IBridger(a.usdtBridger), IBridger(a.usdcBridger));
                console2.log("configured forwarder bridgers");
            }

            if (a.quoter != address(0)) {
                if (quoterTokenIns.length == 0 && quoterAddrs.length == 0) {
                    // Convenience default: configure swaps originating from USDT.
                    f.setQuoter(a.usdt, IQuoter(a.quoter));
                    console2.log("configured forwarder quoter (default tokenIn=USDT)");
                } else {
                    require(quoterTokenIns.length == quoterAddrs.length, "QUOTER_TOKEN_INS/QUOTER_ADDRS mismatch");
                    for (uint256 i = 0; i < quoterTokenIns.length; ++i) {
                        f.setQuoter(quoterTokenIns[i], IQuoter(quoterAddrs[i]));
                    }
                    console2.log("configured forwarder quoter(s)");
                }
            }

            if (forwarderFinalOwner != deployer && forwarderFinalOwner != address(0)) {
                f.transferOwnership(forwarderFinalOwner);
                console2.log("transferred forwarder ownership");
            }
        }

        // Wire: intents fee config
        if (configureIntents) {
            require(a.intents.code.length != 0, "intents not deployed");
            UntronIntents u = UntronIntents(a.intents);
            require(
                u.owner() == deployer, "not intents owner (deploy with INTENTS_OWNER=deployer or broadcast as owner)"
            );

            u.setRecommendedIntentFee(feePpm, feeFlat);
            console2.log("configured intents recommended fee");

            if (intentsFinalOwner != deployer && intentsFinalOwner != address(0)) {
                u.transferOwnership(intentsFinalOwner);
                console2.log("transferred intents ownership");
            }
        }

        console2.log("USDT", a.usdt);
        console2.log("USDC", a.usdc);
        console2.log("UntronV3", a.v3);
        if (a.tronReader != address(0)) console2.log("MockTronTxReader", a.tronReader);
        console2.log("IntentsForwarder", a.forwarder);
        console2.log("UntronIntents", a.intents);
        if (a.quoter != address(0)) console2.log("Quoter", a.quoter);
        if (a.usdtBridger != address(0)) console2.log("USDTBridger", a.usdtBridger);
        if (a.usdcBridger != address(0)) console2.log("USDCBridger", a.usdcBridger);

        vm.stopBroadcast();
    }

    /*//////////////////////////////////////////////////////////////
                               INTERNALS
    //////////////////////////////////////////////////////////////*/

    function _salt(address deployer, uint8 crossChainFlag, uint88 saltId) internal pure returns (bytes32) {
        return bytes32(abi.encodePacked(deployer, bytes1(crossChainFlag), bytes11(saltId)));
    }

    function _deployCreate3IfMissing(string memory label, bytes32 salt, bytes memory initCode)
        internal
        returns (address deployed)
    {
        deployed = computeCreate3Address(salt, msg.sender);
        if (deployed.code.length != 0) {
            console2.log("exists", label, deployed);
            return deployed;
        }

        address created = create3(salt, initCode);
        require(created == deployed, "create3 address mismatch");
        require(created.code.length != 0, "create3 deploy failed");
        vm.label(created, label);
        console2.log("deployed", label, created);
        return created;
    }

    function _deployBridger(
        string memory kind,
        bytes32 salt,
        address owner,
        address authorizedCaller,
        uint256 feeBridgerFee,
        address usdt0Token,
        address usdt0Oft,
        uint256[] memory usdt0SupportedChainIds,
        uint256[] memory usdt0EidsU256,
        address cctpTokenMessengerV2,
        address usdc,
        uint256[] memory cctpSupportedChainIds,
        uint256[] memory cctpDomainsU256
    ) internal returns (address bridger) {
        if (_eq(kind, "existing")) {
            // Resolve by kind.
            revert("existing bridger kind requires *_BRIDGER_ADDRESS; set DEPLOY_*_BRIDGER=false");
        }
        if (_eq(kind, "mock_exact")) {
            return _deployCreate3IfMissing("ExactBridger", salt, type(ExactBridger).creationCode);
        }
        if (_eq(kind, "mock_fee")) {
            bridger = _deployCreate3IfMissing("FeeBridger", salt, type(FeeBridger).creationCode);
            FeeBridger(bridger).setFee(feeBridgerFee);
            return bridger;
        }
        if (_eq(kind, "mock_revert")) {
            return _deployCreate3IfMissing("RevertingBridger", salt, type(RevertingBridger).creationCode);
        }
        if (_eq(kind, "mock_transport")) {
            return _deployCreate3IfMissing("MockTransportBridger", salt, type(MockTransportBridger).creationCode);
        }
        if (_eq(kind, "usdt0")) {
            uint32[] memory eids = _toUint32Array(usdt0EidsU256);
            return _deployCreate3IfMissing(
                "USDT0Bridger",
                salt,
                abi.encodePacked(
                    type(USDT0Bridger).creationCode,
                    abi.encode(owner, authorizedCaller, usdt0Token, usdt0Oft, usdt0SupportedChainIds, eids)
                )
            );
        }
        if (_eq(kind, "cctp")) {
            uint32[] memory domains = _toUint32Array(cctpDomainsU256);
            return _deployCreate3IfMissing(
                "CCTPV2Bridger",
                salt,
                abi.encodePacked(
                    type(CCTPV2Bridger).creationCode,
                    abi.encode(owner, authorizedCaller, cctpTokenMessengerV2, usdc, cctpSupportedChainIds, domains)
                )
            );
        }
        revert("Invalid bridger kind");
    }

    function _toUint32Array(uint256[] memory xs) internal pure returns (uint32[] memory ys) {
        ys = new uint32[](xs.length);
        for (uint256 i = 0; i < xs.length; ++i) {
            require(xs[i] <= type(uint32).max, "uint32 overflow");
            ys[i] = uint32(xs[i]);
        }
    }

    function _eq(string memory a, string memory b) internal pure returns (bool) {
        return keccak256(bytes(a)) == keccak256(bytes(b));
    }
}
