// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IntentsForwarder} from "../IntentsForwarder.sol";

/// @dev Minimal "mintable" interface used by the mock to deliver funds.
interface IMintable {
    function mint(address to, uint256 amount) external;
}

/// @notice Minimal forwarder mock for testing UntronIntents in isolation.
/// @dev Implements `pullFromReceiver` by minting tokens to the requested beneficiary.
contract MockForwarderPuller {
    error RevertPull();
    error BalanceMismatch(uint256 expected, uint256 got);

    struct LastCall {
        uint256 targetChain;
        address beneficiary;
        bool beneficiaryClaimOnly;
        bytes32 intentHash;
        bytes32 forwardSalt;
        uint256 balance;
        address tokenIn;
        address tokenOut;
        bytes32 swapDataHash;
        bytes32 bridgeDataHash;
        uint256 msgValue;
    }

    struct NextPull {
        address token;
        uint256 amount;
        bool shouldRevert;
        bool enforceBalanceParam;
    }

    NextPull public nextPull;
    LastCall public lastCall;

    function setNextPull(address token, uint256 amount) external {
        nextPull.token = token;
        nextPull.amount = amount;
    }

    function setShouldRevert(bool shouldRevert_) external {
        nextPull.shouldRevert = shouldRevert_;
    }

    /// @notice When enabled, require `req.balance != 0` to match `nextPull.amount`.
    function setEnforceBalanceParam(bool enforceBalanceParam_) external {
        nextPull.enforceBalanceParam = enforceBalanceParam_;
    }

    function pullFromReceiver(IntentsForwarder.PullRequest calldata req) external payable returns (uint256 amountOut) {
        NextPull memory cfg = nextPull;
        if (cfg.shouldRevert) revert RevertPull();

        if (cfg.enforceBalanceParam && req.balance != 0 && req.balance != cfg.amount) {
            revert BalanceMismatch(cfg.amount, req.balance);
        }

        address token = cfg.token == address(0) ? req.tokenIn : cfg.token;
        uint256 amount = cfg.amount;

        lastCall = LastCall({
            targetChain: req.targetChain,
            beneficiary: req.beneficiary,
            beneficiaryClaimOnly: req.beneficiaryClaimOnly,
            intentHash: req.intentHash,
            forwardSalt: req.forwardSalt,
            balance: req.balance,
            tokenIn: req.tokenIn,
            tokenOut: req.tokenOut,
            swapDataHash: keccak256(abi.encode(req.swapData)),
            bridgeDataHash: keccak256(req.bridgeData),
            msgValue: msg.value
        });

        IMintable(token).mint(req.beneficiary, amount);
        return amount;
    }
}

