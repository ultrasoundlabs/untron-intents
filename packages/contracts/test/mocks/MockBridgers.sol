// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IBridger} from "../../src/bridgers/interfaces/IBridger.sol";

contract ExactBridger is IBridger {
    address public lastInputToken;
    uint256 public lastInputAmount;
    address public lastOutputAddress;
    uint256 public lastOutputChainId;
    bytes public lastExtraData;
    uint256 public lastMsgValue;

    uint256 public refundToCaller;

    function setRefundToCaller(uint256 refundToCaller_) external {
        refundToCaller = refundToCaller_;
    }

    function bridge(
        address inputToken,
        uint256 inputAmount,
        address outputAddress,
        uint256 outputChainId,
        bytes calldata extraData
    ) external payable returns (uint256 expectedAmountOut) {
        lastInputToken = inputToken;
        lastInputAmount = inputAmount;
        lastOutputAddress = outputAddress;
        lastOutputChainId = outputChainId;
        lastExtraData = extraData;
        lastMsgValue = msg.value;

        if (refundToCaller != 0) {
            (bool success,) = payable(msg.sender).call{value: refundToCaller}("");
            require(success, "REFUND_FAILED");
        }
        return inputAmount;
    }

    receive() external payable {}
}

contract FeeBridger is IBridger {
    uint256 public fee;

    function setFee(uint256 fee_) external {
        fee = fee_;
    }

    function bridge(address, uint256 inputAmount, address, uint256, bytes calldata) external payable returns (uint256) {
        return inputAmount - fee;
    }
}

contract RevertingBridger is IBridger {
    error RevertBridge();

    function bridge(address, uint256, address, uint256, bytes calldata) external payable returns (uint256) {
        revert RevertBridge();
    }
}
