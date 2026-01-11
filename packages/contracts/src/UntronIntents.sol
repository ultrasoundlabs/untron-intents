// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Call} from "./SwapExecutor.sol";
import {IntentsForwarder} from "./IntentsForwarder.sol";
import {IUntronV3} from "./external/interfaces/IUntronV3.sol";
import {ITronTxReader, TriggerSmartContract} from "./external/interfaces/ITronTxReader.sol";
import {TokenUtils} from "./utils/TokenUtils.sol";

import {Ownable} from "solady/auth/Ownable.sol";
import {LibBytes} from "solady/utils/g/LibBytes.sol";
import {ReentrancyGuard} from "solady/utils/ReentrancyGuard.sol";

/// @title Untron Intents
/// @notice Intent-based platform that lets people pay for execution of certain transactions
///         on Tron blockchain using an escrow on an EVM chain and a light client Tron verifier.
/// @author Ultrasound Labs
contract UntronIntents is Ownable, ReentrancyGuard {
    using LibBytes for bytes;

    struct TriggerSmartContractIntent {
        address to;
        bytes data;
    }

    struct USDTTransferIntent {
        address to;
        uint256 amount;
    }

    enum IntentType {
        TRIGGER_SMART_CONTRACT,
        USDT_TRANSFER
    }

    struct Intent {
        IntentType intentType;
        bytes intentSpecs;
        address refundBeneficiary;
        address token;
        uint256 amount;
    }

    struct IntentState {
        Intent intent;
        uint256 solverClaimedAt;
        uint256 deadline;
        address solver;
        bool solved;
        bool funded;
        bool settled;
    }

    error AlreadyExists();
    error InvalidDeadline();
    error AlreadyClaimed();
    error NotClaimed();
    error NotExpiredYet();
    error AlreadySolved();
    error NotSolver();
    error WrongTxProps();
    error TronInvalidTrc20DataLength();
    error TronInvalidCalldataLength();
    error NotATrc20Transfer();
    error IncorrectPullAmount();
    error IntentNotFound();
    error AlreadyFunded();
    error InvalidReceiverAmount();
    error NothingToSettle();

    /// @notice Recommended intent fee in parts-per-million of the intent amount.
    uint256 public recommendedIntentFeePpm;
    /// @notice Recommended intent fee flat component (in the escrow token's units).
    uint256 public recommendedIntentFeeFlat;

    /// @notice Mapping from intent id to intent state.
    mapping(bytes32 => IntentState) public intents;

    /// @notice EVM-chain USDT used as the solver claim deposit token.
    address public immutable USDT;
    /// @notice UntronV3 config contract providing Tron verifier and well-known Tron addresses.
    IUntronV3 public immutable V3;

    /// @notice Grace period after claim before a solver can be unclaimed.
    uint256 public constant TIME_TO_FILL = 2 minutes;
    /// @notice Amount of USDT deposit required to claim an intent.
    uint256 public constant INTENT_CLAIM_DEPOSIT = 1_000_000;
    /// @notice Default deadline duration for receiver-originated intents.
    uint256 public constant RECEIVER_INTENT_DURATION = 1 days;

    /// @dev TRC-20 function selectors.
    bytes4 internal constant _SELECTOR_TRANSFER = bytes4(keccak256("transfer(address,uint256)"));
    // solhint-disable-next-line gas-small-strings
    bytes4 internal constant _SELECTOR_TRANSFER_FROM = bytes4(keccak256("transferFrom(address,address,uint256)"));

    /// @dev UntronController function selectors.
    bytes4 internal constant _SELECTOR_TRANSFER_USDT_FROM_CONTROLLER =
    // solhint-disable-next-line gas-small-strings
    bytes4(keccak256("transferUsdtFromController(address,uint256)"));
    // solhint-disable-next-line gas-small-strings
    bytes4 internal constant _SELECTOR_MULTICALL = bytes4(keccak256("multicall(bytes[])"));

    constructor(address _owner, IUntronV3 v3, address usdt) {
        _initializeOwner(_owner);
        V3 = v3;
        USDT = usdt;
    }

    /// @notice Computes the receiver-originated intent id used by this contract.
    /// @param forwarder Forwarder that owns the receiver.
    /// @param toTron Tron recipient address (raw `0x41 || 20 bytes` cast into `address`).
    /// @param forwardSalt Forwarder salt used for the ephemeral receiver.
    /// @param token Escrow token on this chain.
    /// @param amount Expected receiver balance (ephemeral receiver amount).
    /// @return id Deterministic id for the receiver intent.
    function receiverIntentId(
        IntentsForwarder forwarder,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) public pure returns (bytes32) {
        bytes32 intentHash = keccak256(abi.encode(forwarder, toTron));
        return keccak256(abi.encodePacked(intentHash, forwardSalt, token, amount));
    }

    // admin functions

    /// @notice Updates the fee schedule recommended for receiver-originated intents.
    /// @param ppm Fee in parts-per-million of the amount.
    /// @param flat Flat fee component.
    function setRecommendedIntentFee(uint256 ppm, uint256 flat) external onlyOwner {
        recommendedIntentFeePpm = ppm;
        recommendedIntentFeeFlat = flat;
    }

    // external functions

    /// @notice Creates a new intent by escrowing `intent.amount` of `intent.token`.
    /// @param intent Intent parameters.
    /// @param deadline Unix timestamp after which the intent can be closed/refunded.
    function createIntent(Intent calldata intent, uint256 deadline) external payable {
        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id = keccak256(abi.encodePacked(msg.sender, intentHash, deadline));
        if (intents[id].deadline != 0) revert AlreadyExists();
        if (deadline == 0) revert InvalidDeadline();

        TokenUtils.transferFrom(intent.token, msg.sender, payable(address(this)), intent.amount);

        intents[id] = IntentState(intent, 0, deadline, address(0), false, true, false);
    }

    /// @notice Creates a receiver-originated intent by pulling funds from an IntentsForwarder receiver.
    /// @param forwarder Forwarder that owns the receiver.
    /// @param toTron Tron recipient address (raw `0x41 || 20 bytes` cast into `address`).
    /// @param forwardSalt Forwarder salt used for the ephemeral receiver.
    /// @param token Escrow token on this chain.
    /// @param amount Expected receiver balance (0 = use actual pulled amount).
    function createIntentFromReceiver(
        IntentsForwarder forwarder,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) external payable nonReentrant {
        bytes32 intentHash = keccak256(abi.encode(forwarder, toTron));
        bytes32 id = keccak256(abi.encodePacked(intentHash, forwardSalt, token, amount));
        if (intents[id].deadline != 0) revert AlreadyExists();

        IntentsForwarder.PullRequest memory pull = IntentsForwarder.PullRequest({
            targetChain: block.chainid,
            beneficiary: payable(address(this)),
            beneficiaryClaimOnly: true,
            intentHash: intentHash,
            forwardSalt: forwardSalt,
            balance: amount,
            tokenIn: token,
            tokenOut: token,
            swapData: new Call[](0),
            bridgeData: new bytes(0)
        });

        uint256 preBalance = TokenUtils.getBalanceOf(token, address(this));
        forwarder.pullFromReceiver(pull);
        uint256 postBalance = TokenUtils.getBalanceOf(token, address(this));
        uint256 pulledAmount = postBalance - preBalance;

        if (amount == 0) {
            amount = pulledAmount;
        } else if (amount != pulledAmount) {
            revert IncorrectPullAmount();
        }

        Intent memory intent = Intent({
            intentType: IntentType.USDT_TRANSFER,
            intentSpecs: abi.encode(
                USDTTransferIntent({
                    to: toTron,
                    // this entire function of course assumes provided forwarder has pulled a dollar stablecoin;
                    // otherwise the user of that forwarder can get rugged (skill issue)
                    amount: amount - recommendedIntentFee(amount)
                })
            ),
            refundBeneficiary: owner(),
            token: token,
            amount: amount
        });

        intents[id] = IntentState(intent, 0, block.timestamp + RECEIVER_INTENT_DURATION, address(0), false, true, false);
    }

    /// @notice Creates a receiver-originated (ephemeral-only) intent and claims it in one transaction.
    /// @dev This enables "virtual" intents: solvers can claim + prove the Tron fill before the escrow
    ///      is pulled into this contract, then settle once the in-flight funds arrive.
    ///
    /// Requirements:
    /// - `amount != 0` (ephemeral receiver mode only).
    /// - The intent id must not already exist.
    ///
    /// Notes:
    /// - The required Tron payment amount is snapshotted at claim time by storing it in `intentSpecs`.
    ///   This makes the intent independent from later changes to `recommendedIntentFee*`.
    /// @param forwarder Forwarder that owns the receiver.
    /// @param toTron Tron recipient address (raw `0x41 || 20 bytes` cast into `address`).
    /// @param forwardSalt Forwarder salt used for the ephemeral receiver.
    /// @param token Escrow token on this chain.
    /// @param amount Expected receiver balance (must be non-zero).
    function claimVirtualReceiverIntent(
        IntentsForwarder forwarder,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) external {
        if (amount == 0) revert InvalidReceiverAmount();

        bytes32 id = receiverIntentId(forwarder, toTron, forwardSalt, token, amount);
        if (intents[id].deadline != 0) revert AlreadyExists();

        TokenUtils.transferFrom(USDT, msg.sender, payable(address(this)), INTENT_CLAIM_DEPOSIT);

        Intent memory intent = Intent({
            intentType: IntentType.USDT_TRANSFER,
            intentSpecs: abi.encode(
                USDTTransferIntent({
                    to: toTron,
                    // Fee is snapshotted at intent creation (claim) time.
                    amount: amount - recommendedIntentFee(amount)
                })
            ),
            refundBeneficiary: owner(),
            token: token,
            amount: amount
        });

        intents[id] = IntentState(
            intent, block.timestamp, block.timestamp + RECEIVER_INTENT_DURATION, msg.sender, false, false, false
        );
    }

    /// @notice Claims an existing intent by posting the solver deposit.
    /// @param id Intent id.
    function claimIntent(bytes32 id) external {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (intents[id].solver != address(0)) revert AlreadyClaimed();

        TokenUtils.transferFrom(USDT, msg.sender, payable(address(this)), INTENT_CLAIM_DEPOSIT);

        intents[id].solver = msg.sender;
        intents[id].solverClaimedAt = block.timestamp;
    }

    /// @notice Clears the solver claim after a timeout if the intent is still unsolved.
    /// @param id Intent id.
    function unclaimIntent(bytes32 id) external {
        IntentState storage st = intents[id];
        if (st.deadline == 0) revert IntentNotFound();
        if (st.solver == address(0)) revert NotClaimed();
        if (block.timestamp < st.solverClaimedAt + TIME_TO_FILL) revert NotExpiredYet();
        if (st.solved) revert AlreadySolved();

        address solver_ = st.solver;
        bool funded_ = st.funded;
        address refundBeneficiary = st.intent.refundBeneficiary;

        st.solver = address(0);
        st.solverClaimedAt = 0;

        if (!funded_) {
            TokenUtils.transfer(USDT, payable(solver_), INTENT_CLAIM_DEPOSIT);
        } else {
            TokenUtils.transfer(USDT, payable(msg.sender), INTENT_CLAIM_DEPOSIT / 2);
            TokenUtils.transfer(USDT, payable(refundBeneficiary), INTENT_CLAIM_DEPOSIT / 2);
        }
    }

    /// @notice Proves that the solver executed the intent on Tron and marks it solved.
    /// @param id Intent id.
    /// @param blocks 20 Tron block headers for the verifier.
    /// @param encodedTx Protobuf-encoded Tron Transaction bytes.
    /// @param proof Merkle proof for the transaction inclusion.
    /// @param index Leaf index in the transaction tree.
    function proveIntentFill(
        bytes32 id,
        bytes[20] calldata blocks,
        bytes calldata encodedTx,
        bytes32[] calldata proof,
        uint256 index
    ) external nonReentrant {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (intents[id].solver != msg.sender) revert NotSolver();
        if (intents[id].solved) revert AlreadySolved();

        ITronTxReader reader = V3.tronReader();
        TriggerSmartContract memory tronTx = reader.readTriggerSmartContract(blocks, encodedTx, proof, index);

        if (intents[id].intent.intentType == IntentType.TRIGGER_SMART_CONTRACT) {
            TriggerSmartContractIntent memory intent =
                abi.decode(intents[id].intent.intentSpecs, (TriggerSmartContractIntent));
            if (address(uint160(uint168(tronTx.toTron))) != intent.to || !intent.data.eq(tronTx.data)) {
                revert WrongTxProps();
            }
        } else if (intents[id].intent.intentType == IntentType.USDT_TRANSFER) {
            USDTTransferIntent memory intent = abi.decode(intents[id].intent.intentSpecs, (USDTTransferIntent));
            USDTTransferIntent memory parsedOperation = _dispatchTronTxCall(tronTx);

            if (parsedOperation.to != intent.to || parsedOperation.amount != intent.amount) {
                revert WrongTxProps();
            }
        }

        intents[id].solved = true;
        intents[id].solverClaimedAt = block.timestamp;

        _settleIfPossible(id);
    }

    /// @notice Pulls the escrow for an existing receiver intent into this contract (then settles if solved).
    /// @param forwarder Forwarder that owns the receiver.
    /// @param toTron Tron recipient address.
    /// @param forwardSalt Forwarder salt used for the ephemeral receiver.
    /// @param token Escrow token on this chain.
    /// @param amount Expected receiver balance.
    function fundReceiverIntent(
        IntentsForwarder forwarder,
        address toTron,
        bytes32 forwardSalt,
        address token,
        uint256 amount
    ) external payable nonReentrant {
        bytes32 id = receiverIntentId(forwarder, toTron, forwardSalt, token, amount);
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (intents[id].funded) revert AlreadyFunded();

        IntentsForwarder.PullRequest memory pull = IntentsForwarder.PullRequest({
            targetChain: block.chainid,
            beneficiary: payable(address(this)),
            beneficiaryClaimOnly: true,
            intentHash: keccak256(abi.encode(forwarder, toTron)),
            forwardSalt: forwardSalt,
            balance: amount,
            tokenIn: token,
            tokenOut: token,
            swapData: new Call[](0),
            bridgeData: new bytes(0)
        });

        uint256 preBalance = TokenUtils.getBalanceOf(token, address(this));
        forwarder.pullFromReceiver{value: msg.value}(pull);
        uint256 postBalance = TokenUtils.getBalanceOf(token, address(this));
        uint256 pulledAmount = postBalance - preBalance;

        if (pulledAmount != amount) revert IncorrectPullAmount();

        intents[id].funded = true;
        _settleIfPossible(id);
    }

    /// @notice Settles a solved+funded intent by paying the solver (deposit + escrow).
    /// @param id Intent id.
    function settleIntent(bytes32 id) external nonReentrant {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (!_settleIfPossible(id)) revert NothingToSettle();
    }

    /// @notice Closes an expired intent and refunds escrow (and/or releases deposits).
    /// @param id Intent id.
    function closeIntent(bytes32 id) external {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (intents[id].deadline > block.timestamp) revert NotExpiredYet();

        // If escrow is funded and the intent was solved, settle instead of closing.
        if (intents[id].solved) {
            if (intents[id].funded && !intents[id].settled) {
                _settleIfPossible(id);
                return;
            }
            // If the intent was solved but escrow never arrived, let the solver recover the deposit and delete.
            if (!intents[id].funded && intents[id].solver != address(0)) {
                TokenUtils.transfer(USDT, payable(intents[id].solver), INTENT_CLAIM_DEPOSIT);
            }
            delete intents[id];
            return;
        }

        // If escrow was funded, refund it to the refund beneficiary.
        if (intents[id].funded) {
            TokenUtils.transfer(
                intents[id].intent.token, payable(intents[id].intent.refundBeneficiary), intents[id].intent.amount
            );
        }

        // If a solver claimed, release their deposit. If escrow was funded, keep the existing "penalty" split;
        // otherwise refund the solver in full (nothing was ever available to execute against).
        if (intents[id].solver != address(0)) {
            if (intents[id].funded) {
                TokenUtils.transfer(USDT, payable(msg.sender), INTENT_CLAIM_DEPOSIT / 2);
                TokenUtils.transfer(USDT, payable(intents[id].intent.refundBeneficiary), INTENT_CLAIM_DEPOSIT / 2);
            } else {
                TokenUtils.transfer(USDT, payable(intents[id].solver), INTENT_CLAIM_DEPOSIT);
            }
        }

        delete intents[id];
    }

    // public functions

    /// @notice Computes the recommended fee for an intent amount.
    /// @param amount Escrow amount.
    /// @return fee Recommended fee.
    function recommendedIntentFee(uint256 amount) public view returns (uint256) {
        return amount * recommendedIntentFeePpm / 1_000_000 + recommendedIntentFeeFlat;
    }

    // internal functions

    function _settleIfPossible(bytes32 id) internal returns (bool settledNow) {
        IntentState storage st = intents[id];
        if (st.settled || !st.solved || !st.funded) return false;

        st.settled = true;
        TokenUtils.transfer(USDT, payable(st.solver), INTENT_CLAIM_DEPOSIT);
        TokenUtils.transfer(st.intent.token, payable(st.solver), st.intent.amount);
        return true;
    }

    function _dispatchTronTxCall(TriggerSmartContract memory tronTx)
        internal
        view
        returns (USDTTransferIntent memory op)
    {
        address tronUsdt = V3.tronUsdt();
        address controller = V3.CONTROLLER_ADDRESS();

        address tronTo = address(uint160(uint168(tronTx.toTron)));
        bytes memory data = tronTx.data;

        if (data.length < 4) revert TronInvalidCalldataLength();
        // forge-lint: disable-next-line(unsafe-typecast)
        bytes4 sig = bytes4(data);

        if (tronTo == tronUsdt) {
            if (sig == _SELECTOR_TRANSFER) {
                (op.to, op.amount) = _decodeTrc20TransferArgs(data);
            } else if (sig == _SELECTOR_TRANSFER_FROM) {
                (, op.to, op.amount) = _decodeTrc20TransferFromArgs(data);
            } else {
                revert NotATrc20Transfer();
            }
        } else if (tronTo == controller) {
            if (sig == _SELECTOR_TRANSFER_USDT_FROM_CONTROLLER) {
                (op.to, op.amount) = _decodeTrc20TransferArgs(data);
            } else if (sig == _SELECTOR_MULTICALL) {
                // TODO: implement
                revert NotATrc20Transfer();
            } else {
                revert NotATrc20Transfer();
            }
        }
    }

    /// @notice Decode TRC-20 `transfer(address,uint256)` calldata arguments.
    /// @dev Expects exact calldata length: `4 + 32*2`.
    /// @param data ABI-encoded calldata for `transfer(address,uint256)`.
    /// @return to The Tron raw recipient address (`0x41 || 20 bytes`).
    /// @return amount The transfer amount.
    function _decodeTrc20TransferArgs(bytes memory data) internal pure returns (address to, uint256 amount) {
        uint256 dataEnd = data.length;
        if (dataEnd != 4 + 32 * 2) revert TronInvalidTrc20DataLength();
        bytes32 word1;
        bytes32 word2;
        // solhint-disable-next-line no-inline-assembly
        assembly ("memory-safe") {
            word1 := mload(add(data, 0x24)) // 0x20 (data) + 4 (selector)
            word2 := mload(add(data, 0x44)) // 0x20 (data) + 36
        }
        to = address(uint160(uint256(word1)));
        amount = uint256(word2);
    }

    /// @notice Decode TRC-20 `transferFrom(address,address,uint256)` calldata arguments.
    /// @dev Expects exact calldata length: `4 + 32*3`.
    /// @param data ABI-encoded calldata for `transferFrom(address,address,uint256)`.
    /// @return from The Tron raw source address (`0x41 || 20 bytes`).
    /// @return to The Tron raw destination address (`0x41 || 20 bytes`).
    /// @return amount The transfer amount.
    function _decodeTrc20TransferFromArgs(bytes memory data)
        internal
        pure
        returns (address from, address to, uint256 amount)
    {
        uint256 dataEnd = data.length;
        if (dataEnd != 4 + 32 * 3) revert TronInvalidTrc20DataLength();
        bytes32 w1;
        bytes32 w2;
        bytes32 w3;
        // solhint-disable-next-line no-inline-assembly
        assembly ("memory-safe") {
            w1 := mload(add(data, 0x24)) // from
            w2 := mload(add(data, 0x44)) // to
            w3 := mload(add(data, 0x64)) // amount
        }
        from = address(uint160(uint256(w1)));
        to = address(uint160(uint256(w2)));
        amount = uint256(w3);
    }

    /// @notice Decode UntronController `transferUsdtFromController(address,uint256)` calldata arguments.
    /// @dev Expects exact calldata length: `4 + 32*2`.
    /// @param data ABI-encoded calldata for `transferUsdtFromController(address,uint256)`.
    /// @return to The Tron raw recipient address (`0x41 || 20 bytes`).
    /// @return amount The transfer amount.
    function _decodeControllerTransferArgs(bytes memory data) internal pure returns (address to, uint256 amount) {
        uint256 dataEnd = data.length;
        if (dataEnd != 4 + 32 * 2) revert TronInvalidTrc20DataLength();
        bytes32 word1;
        bytes32 word2;
        // solhint-disable-next-line no-inline-assembly
        assembly ("memory-safe") {
            word1 := mload(add(data, 0x24)) // 0x20 (data) + 4 (selector)
            word2 := mload(add(data, 0x44)) // 0x20 (data) + 36
        }
        to = address(uint160(uint256(word1)));
        amount = uint256(word2);
    }
}
