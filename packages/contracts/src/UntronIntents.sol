// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Call} from "./SwapExecutor.sol";
import {IntentsForwarder} from "./IntentsForwarder.sol";
import {IUntronV3} from "./external/interfaces/IUntronV3.sol";
import {ITronTxReader, TriggerSmartContract} from "./external/interfaces/ITronTxReader.sol";
import {TokenUtils} from "./utils/TokenUtils.sol";

import {LibBytes} from "solady/utils/g/LibBytes.sol";
import {ReentrancyGuard} from "solady/utils/ReentrancyGuard.sol";

import {UntronIntentsIndexedOwnable} from "./auth/UntronIntentsIndexedOwnable.sol";

/// @title Untron Intents
/// @notice Intent-based platform that lets people pay for execution of certain transactions
///         on Tron blockchain using an escrow on an EVM chain and a light client Tron verifier.
/// @dev High-level model
/// Actors:
/// - Maker / user: posts an intent and (usually) escrows funds on this chain.
/// - Solver: executes the requested action on Tron and gets paid from escrow.
/// - Refund beneficiary: receives escrow refunds (and possibly penalty proceeds) if the intent expires.
///
/// The contract intentionally does not “execute Tron”:
/// - Solvers submit a *proof* of a Tron transaction (via {ITronTxReader}) to mark an intent as solved.
/// - Once solved+funded, settlement on this chain is deterministic: pay solver escrow + return solver deposit.
///
/// Two main flows exist:
/// 1) Maker-funded intent:
///    - Maker calls {createIntent} and escrows `intent.amount` of `intent.token` on this chain.
///    - A solver calls {claimIntent} by posting a fixed USDT deposit.
///    - The solver executes the corresponding action on Tron and calls {proveIntentFill}.
///    - Contract settles immediately if escrow is present.
///
/// 2) Receiver-originated intent (cross-chain / “forwarder receiver” model):
///    - Funds are sent to a deterministic {UntronReceiver} address owned by an {IntentsForwarder}.
///    - Anyone can pull those funds into this contract via {IntentsForwarder.pullFromReceiver}.
///    - {createIntentFromReceiver} creates a USDT-transfer intent using the pulled amount.
///    - Alternatively, {claimVirtualReceiverIntent} allows the solver to claim+prove *before* funding
///      (creating a “virtual” intent), then {fundReceiverIntent} later pulls the escrow and triggers settlement.
///
/// Invariants / important properties:
/// - Solvers can only prove fills for intents they currently own (the `solver` address).
/// - Settlement occurs at most once (`settled` flag).
/// - If an intent expires without being solved, escrow is refunded and the solver deposit is either refunded
///   (unfunded intents) or split as a penalty (funded intents).
/// @author Ultrasound Labs
contract UntronIntents is UntronIntentsIndexedOwnable, ReentrancyGuard {
    using LibBytes for bytes;

    /*//////////////////////////////////////////////////////////////
                                INTENTS
    //////////////////////////////////////////////////////////////*/

    /// @notice Intent to execute an arbitrary Tron `TriggerSmartContract` call.
    /// @dev The proof must show a Tron transaction whose:
    /// - `toTron` address matches `to`, and
    /// - `data` bytes match exactly.
    struct TriggerSmartContractIntent {
        address to;
        bytes data;
    }

    /// @notice Intent to perform a Tron USDT transfer (or controller-mediated USDT transfer).
    /// @dev The proof must show a Tron transaction whose decoded `(to, amount)` matches.
    struct USDTTransferIntent {
        address to;
        uint256 amount;
    }

    /// @notice Supported intent variants.
    /// @dev This contract is intentionally narrow: it only validates “what happened on Tron”
    /// for these specific patterns, and never attempts to run arbitrary logic itself.
    enum IntentType {
        TRIGGER_SMART_CONTRACT,
        USDT_TRANSFER
    }

    /// @notice Canonical intent parameters.
    /// @param intentType What the solver must do on Tron.
    /// @param intentSpecs ABI-encoded payload depending on `intentType`.
    /// @param refundBeneficiary Receives escrow refunds and, in some cases, penalty proceeds.
    /// @param token Escrow token on this chain (address(0) = native token).
    /// @param amount Escrowed amount on this chain.
    struct Intent {
        IntentType intentType;
        bytes intentSpecs;
        address refundBeneficiary;
        address token;
        uint256 amount;
    }

    /// @notice Full onchain state for an intent id.
    /// @dev The lifecycle is:
    /// - created -> (optionally claimed) -> solved (via proof) -> funded (if not already) -> settled -> deleted
    /// Not all transitions are strictly ordered due to “virtual” receiver intents.
    struct IntentState {
        Intent intent;
        /// @notice Timestamp when the solver last claimed or proved (used for `unclaimIntent` timeout).
        uint256 solverClaimedAt;
        /// @notice Timestamp after which anyone can close the intent and distribute funds.
        uint256 deadline;
        /// @notice Current solver that holds the claim (required for proving the Tron fill).
        address solver;
        /// @notice Whether a valid Tron fill was proven for this intent.
        bool solved;
        /// @notice Whether escrow funds are currently held by this contract.
        bool funded;
        /// @notice Whether settlement already occurred (escrow + deposit paid to solver).
        bool settled;
    }

    /*//////////////////////////////////////////////////////////////
                                 ERRORS
    //////////////////////////////////////////////////////////////*/

    /// @notice Reverts when attempting to create an intent whose id already exists.
    error AlreadyExists();
    /// @notice Reverts when creating an intent with `deadline == 0`.
    error InvalidDeadline();
    /// @notice Reverts when claiming an intent that is already claimed.
    error AlreadyClaimed();
    /// @notice Reverts when attempting to unclaim an intent with no current solver.
    error NotClaimed();
    /// @notice Reverts when a timeout/expiry has not yet elapsed.
    error NotExpiredYet();
    /// @notice Reverts when attempting an action that requires the intent to be unsolved.
    error AlreadySolved();
    /// @notice Reverts when a caller is not the current solver for an intent.
    error NotSolver();
    /// @notice Reverts when a proven Tron tx does not match the intent specs.
    error WrongTxProps();
    /// @notice Reverts when TRC-20 calldata length is not the expected ABI-encoded size.
    error TronInvalidTrc20DataLength();
    /// @notice Reverts when calldata is too short to even contain a function selector.
    error TronInvalidCalldataLength();
    /// @notice Reverts when a proven Tron tx cannot be interpreted as an allowed USDT transfer.
    error NotATrc20Transfer();
    /// @notice Reverts when pulled amount from receiver doesn't match `amount` (when specified).
    error IncorrectPullAmount();
    /// @notice Reverts when an intent id does not exist.
    error IntentNotFound();
    /// @notice Reverts when attempting to fund a receiver intent that is already funded.
    error AlreadyFunded();
    /// @notice Reverts when a receiver/virtual intent requires non-zero amount but amount is 0.
    error InvalidReceiverAmount();
    /// @notice Reverts when calling {settleIntent} but intent is not in a settleable state.
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
    /// @dev Denominated in this chain's USDT token units (e.g. 6 decimals for typical USDT).
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
    /// @dev Receiver-originated ids are derived from forwarder/receiver parameters, so that
    /// offchain agents can predict them without depending on the caller address.
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
        _emitRecommendedIntentFeeSet(ppm, flat);
    }

    // external functions

    /// @notice Creates a new intent by escrowing `intent.amount` of `intent.token`.
    /// @dev Maker-funded intent ids are derived from `(creator, intentHash, deadline)`:
    /// - `intentHash = keccak256(abi.encode(intent))`
    /// - `id = keccak256(abi.encodePacked(msg.sender, intentHash, deadline))`
    /// This gives the maker a deterministic id for a given parameterization and deadline.
    /// @param intent Intent parameters.
    /// @param deadline Unix timestamp after which the intent can be closed/refunded.
    function createIntent(Intent calldata intent, uint256 deadline) external payable {
        bytes32 intentHash = keccak256(abi.encode(intent));
        bytes32 id = keccak256(abi.encodePacked(msg.sender, intentHash, deadline));
        if (intents[id].deadline != 0) revert AlreadyExists();
        if (deadline == 0) revert InvalidDeadline();

        TokenUtils.transferFrom(intent.token, msg.sender, payable(address(this)), intent.amount);

        intents[id] = IntentState(intent, 0, deadline, address(0), false, true, false);

        _emitIntentCreated(
            id,
            msg.sender,
            uint8(intent.intentType),
            intent.token,
            intent.amount,
            intent.refundBeneficiary,
            deadline,
            intent.intentSpecs
        );
        _emitIntentFunded(id, msg.sender, intent.token, intent.amount);
    }

    // solhint-disable function-max-lines

    /// @notice Creates a receiver-originated intent by pulling funds from an IntentsForwarder receiver.
    /// @dev This flow assumes the forwarder pulls a dollar stablecoin that matches the Tron USDT
    /// denomination the solver will pay out on Tron. If the forwarder is misconfigured to pull some
    /// other token, the intent creator can get rugged (by design, this contract cannot validate it).
    ///
    /// Fee model:
    /// - The contract stores the *escrowed amount* as `intent.amount`.
    /// - The Tron payment amount is set to `amount - recommendedIntentFee(amount)` and stored in `intentSpecs`.
    /// - `recommendedIntentFee*` is advisory/suggested; it is up to integrators to decide fee policy.
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
        uint256 amountParam = amount;
        uint256 deadline = block.timestamp + RECEIVER_INTENT_DURATION;
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

        intents[id] = IntentState(intent, 0, deadline, address(0), false, true, false);

        _emitReceiverIntentParams(id, address(forwarder), toTron, forwardSalt, token, amountParam);
        _emitReceiverIntentFeeSnap(
            id, recommendedIntentFeePpm, recommendedIntentFeeFlat, amount - recommendedIntentFee(amount)
        );
        _emitIntentCreated(
            id,
            msg.sender,
            uint8(intent.intentType),
            intent.token,
            intent.amount,
            intent.refundBeneficiary,
            deadline,
            intent.intentSpecs
        );
        _emitIntentFunded(id, msg.sender, intent.token, intent.amount);
    }

    // solhint-enable function-max-lines

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

        uint256 deadline = block.timestamp + RECEIVER_INTENT_DURATION;
        bytes32 id = receiverIntentId(forwarder, toTron, forwardSalt, token, amount);
        if (intents[id].deadline != 0) revert AlreadyExists();

        TokenUtils.transferFrom(USDT, msg.sender, payable(address(this)), INTENT_CLAIM_DEPOSIT);

        uint256 feePpm = recommendedIntentFeePpm;
        uint256 feeFlat = recommendedIntentFeeFlat;
        uint256 tronPaymentAmount = amount - (amount * feePpm / 1_000_000 + feeFlat);

        Intent memory intent = Intent({
            intentType: IntentType.USDT_TRANSFER,
            intentSpecs: abi.encode(
                USDTTransferIntent({
                    to: toTron,
                    // Fee is snapshotted at intent creation (claim) time.
                    amount: tronPaymentAmount
                })
            ),
            refundBeneficiary: owner(),
            token: token,
            amount: amount
        });

        intents[id] = IntentState(intent, block.timestamp, deadline, msg.sender, false, false, false);

        _emitReceiverIntentParams(id, address(forwarder), toTron, forwardSalt, token, amount);
        _emitReceiverIntentFeeSnap(id, feePpm, feeFlat, tronPaymentAmount);
        _emitIntentCreated(
            id,
            msg.sender,
            uint8(intent.intentType),
            intent.token,
            intent.amount,
            intent.refundBeneficiary,
            deadline,
            intent.intentSpecs
        );
        _emitIntentClaimed(id, msg.sender, INTENT_CLAIM_DEPOSIT);
    }

    /// @notice Claims an existing intent by posting the solver deposit.
    /// @dev Claiming does not guarantee the solver can be paid: it only grants the exclusive right
    /// to submit the Tron proof. The deposit:
    /// - is returned upon successful settlement, OR
    /// - can be penalized if the solver stalls on a funded intent (see {unclaimIntent}/{closeIntent}).
    /// @param id Intent id.
    function claimIntent(bytes32 id) external {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (intents[id].solver != address(0)) revert AlreadyClaimed();

        TokenUtils.transferFrom(USDT, msg.sender, payable(address(this)), INTENT_CLAIM_DEPOSIT);

        intents[id].solver = msg.sender;
        intents[id].solverClaimedAt = block.timestamp;
        _emitIntentClaimed(id, msg.sender, INTENT_CLAIM_DEPOSIT);
    }

    /// @notice Clears the solver claim after a timeout if the intent is still unsolved.
    /// @dev Deposit handling:
    /// - If the intent is *unfunded*, the solver deposit is refunded in full (nothing was executable against).
    /// - If the intent is *funded*, the deposit is split 50/50 between:
    ///   - `msg.sender` (the unclaim caller), and
    ///   - `refundBeneficiary` (the maker / protocol recipient)
    /// This creates a permissionless “keepalive” incentive to clear stale claims.
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

        uint256 depositToCaller;
        uint256 depositToRefundBeneficiary;
        uint256 depositToPrevSolver;
        if (!funded_) {
            depositToPrevSolver = INTENT_CLAIM_DEPOSIT;
            TokenUtils.transfer(USDT, payable(solver_), INTENT_CLAIM_DEPOSIT);
        } else {
            depositToCaller = INTENT_CLAIM_DEPOSIT / 2;
            depositToRefundBeneficiary = INTENT_CLAIM_DEPOSIT / 2;
            TokenUtils.transfer(USDT, payable(msg.sender), INTENT_CLAIM_DEPOSIT / 2);
            TokenUtils.transfer(USDT, payable(refundBeneficiary), INTENT_CLAIM_DEPOSIT / 2);
        }

        _emitIntentUnclaimed(
            id, msg.sender, solver_, funded_, depositToCaller, depositToRefundBeneficiary, depositToPrevSolver
        );
    }

    /// @notice Proves that the solver executed the intent on Tron and marks it solved.
    /// @dev Proof verification is delegated to `V3.tronReader()`:
    /// - This contract assumes the reader correctly verifies block headers, inclusion proofs, and decoding.
    /// - This contract then enforces intent-specific matching on the parsed `TriggerSmartContract`.
    ///
    /// Tron address encoding notes:
    /// - The reader returns Tron “raw” addresses as `bytes21` (`0x41 || 20 bytes`).
    /// - Throughout this contract, such addresses are represented as `address` by dropping the leading `0x41`.
    ///   This is why comparisons use `address(uint160(uint168(bytes21)))`.
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
        // Refresh the "claimed at" timestamp so `TIME_TO_FILL` remains meaningful for any follow-up actions.
        intents[id].solverClaimedAt = block.timestamp;

        _emitIntentSolved(id, msg.sender, tronTx.txId, tronTx.tronBlockNumber);
        _settleIfPossible(id);
    }

    /// @notice Pulls the escrow for an existing receiver intent into this contract (then settles if solved).
    /// @dev Used for “virtual” receiver intents:
    /// - A solver can prove the Tron fill first (setting `solved=true` while `funded=false`).
    /// - Later, anyone can pull the escrow here via the forwarder and trigger settlement.
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
        _emitIntentFunded(id, msg.sender, token, pulledAmount);
        _settleIfPossible(id);
    }

    /// @notice Settles a solved+funded intent by paying the solver (deposit + escrow).
    /// @param id Intent id.
    function settleIntent(bytes32 id) external nonReentrant {
        if (intents[id].deadline == 0) revert IntentNotFound();
        if (!_settleIfPossible(id)) revert NothingToSettle();
    }

    // solhint-disable function-max-lines

    /// @notice Closes an expired intent and refunds escrow (and/or releases deposits).
    /// @dev Anyone can close once `deadline` passes. Closing always deletes the intent state.
    ///
    /// Solved intents:
    /// - If solved+funded but not settled, we settle instead.
    /// - If solved but unfunded (virtual intent never funded), the solver is refunded their deposit.
    ///
    /// Unsolved intents:
    /// - If funded, escrow is refunded to `refundBeneficiary`.
    /// - Solver deposit is:
    ///   - split 50/50 between closer and `refundBeneficiary` if funded, OR
    ///   - refunded in full to solver if unfunded.
    /// @param id Intent id.
    function closeIntent(bytes32 id) external {
        IntentState storage st = intents[id];
        if (st.deadline == 0) revert IntentNotFound();
        if (st.deadline > block.timestamp) revert NotExpiredYet();

        uint256 depositToSolver;

        // If escrow is funded and the intent was solved, settle instead of closing.
        if (st.solved) {
            if (st.funded && !st.settled) {
                _settleIfPossible(id);
                return;
            }

            if (!st.funded && st.solver != address(0)) {
                depositToSolver = INTENT_CLAIM_DEPOSIT;
                TokenUtils.transfer(USDT, payable(st.solver), INTENT_CLAIM_DEPOSIT);
            }

            _emitIntentClosed(
                id,
                msg.sender,
                true,
                st.funded,
                st.settled,
                st.intent.refundBeneficiary,
                st.intent.token,
                0,
                USDT,
                0,
                0,
                depositToSolver
            );

            delete intents[id];
            return;
        }

        uint256 escrowRefunded;
        uint256 depositToCaller;
        uint256 depositToRefundBeneficiary;

        // If escrow was funded, refund it to the refund beneficiary.
        if (st.funded) {
            escrowRefunded = st.intent.amount;
            TokenUtils.transfer(st.intent.token, payable(st.intent.refundBeneficiary), st.intent.amount);
        }

        // If a solver claimed, release their deposit. If escrow was funded, keep the existing "penalty" split;
        // otherwise refund the solver in full (nothing was ever available to execute against).
        if (st.solver != address(0)) {
            if (st.funded) {
                depositToCaller = INTENT_CLAIM_DEPOSIT / 2;
                depositToRefundBeneficiary = INTENT_CLAIM_DEPOSIT / 2;
                TokenUtils.transfer(USDT, payable(msg.sender), INTENT_CLAIM_DEPOSIT / 2);
                TokenUtils.transfer(USDT, payable(st.intent.refundBeneficiary), INTENT_CLAIM_DEPOSIT / 2);
            } else {
                depositToSolver = INTENT_CLAIM_DEPOSIT;
                TokenUtils.transfer(USDT, payable(st.solver), INTENT_CLAIM_DEPOSIT);
            }
        }

        _emitIntentClosed(
            id,
            msg.sender,
            false,
            st.funded,
            st.settled,
            st.intent.refundBeneficiary,
            st.intent.token,
            escrowRefunded,
            USDT,
            depositToCaller,
            depositToRefundBeneficiary,
            depositToSolver
        );

        delete intents[id];
    }

    // solhint-enable function-max-lines

    // public functions

    /// @notice Computes the recommended fee for an intent amount.
    /// @param amount Escrow amount.
    /// @return fee Recommended fee.
    function recommendedIntentFee(uint256 amount) public view returns (uint256) {
        return amount * recommendedIntentFeePpm / 1_000_000 + recommendedIntentFeeFlat;
    }

    // internal functions

    /// @dev Settles and pays the solver if (and only if) the intent is solved and funded.
    /// This function is safe to call opportunistically after either:
    /// - proving the Tron fill (may settle immediately for funded intents), or
    /// - funding escrow for a previously solved virtual intent.
    function _settleIfPossible(bytes32 id) internal returns (bool settledNow) {
        IntentState storage st = intents[id];
        if (st.settled || !st.solved || !st.funded) return false;

        st.settled = true;
        _emitIntentSettled(id, st.solver, st.intent.token, st.intent.amount, USDT, INTENT_CLAIM_DEPOSIT);
        TokenUtils.transfer(USDT, payable(st.solver), INTENT_CLAIM_DEPOSIT);
        TokenUtils.transfer(st.intent.token, payable(st.solver), st.intent.amount);
        return true;
    }

    /// @dev Interprets the parsed Tron transaction as a USDT transfer.
    /// Supported patterns:
    /// - Direct USDT TRC-20 `transfer` and `transferFrom` on `V3.tronUsdt()`.
    /// - `V3.CONTROLLER_ADDRESS()` helper `transferUsdtFromController`.
    /// - `V3.CONTROLLER_ADDRESS()` Solady-style `multicall(bytes[])` that contains exactly one
    ///   `transferUsdtFromController` call (the only call we treat as the “payment”).
    ///
    /// Any other call shapes revert with {NotATrc20Transfer}.
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
                bytes[] memory calls = abi.decode(data.slice(4), (bytes[]));
                bool found;

                for (uint256 i = 0; i < calls.length; ++i) {
                    bytes memory inner = calls[i];
                    if (inner.length < 4) continue;
                    // forge-lint: disable-next-line(unsafe-typecast)
                    bytes4 innerSig = bytes4(inner);

                    if (innerSig == _SELECTOR_TRANSFER_USDT_FROM_CONTROLLER) {
                        if (found) revert NotATrc20Transfer();
                        (op.to, op.amount) = _decodeControllerTransferArgs(inner);
                        found = true;
                    }
                }

                if (!found) revert NotATrc20Transfer();
            } else {
                revert NotATrc20Transfer();
            }
        } else {
            revert NotATrc20Transfer();
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
