// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IBridger} from "../../src/bridgers/interfaces/IBridger.sol";
import {MockERC20} from "./MockERC20.sol";

/// @notice Test-only "transport" bridger that records a bridge request and lets the test
///         deliver funds to the destination address later.
/// @dev This intentionally does not attempt to model source-chain token movement/fees.
contract MockTransportBridger is IBridger {
    struct Pending {
        address inputToken;
        uint256 inputAmount;
        address outputAddress;
        uint256 outputChainId;
        bytes extraData;
        bool delivered;
    }

    Pending public last;

    function bridge(
        address inputToken,
        uint256 inputAmount,
        address outputAddress,
        uint256 outputChainId,
        bytes calldata extraData
    ) external payable returns (uint256 expectedAmountOut) {
        last = Pending({
            inputToken: inputToken,
            inputAmount: inputAmount,
            outputAddress: outputAddress,
            outputChainId: outputChainId,
            extraData: extraData,
            delivered: false
        });
        return inputAmount;
    }

    function deliverLast() external {
        require(!last.delivered, "ALREADY_DELIVERED");
        last.delivered = true;
        MockERC20(last.inputToken).mint(last.outputAddress, last.inputAmount);
    }
}

