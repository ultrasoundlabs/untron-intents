// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {IQuoter} from "../quoters/interfaces/IQuoter.sol";

contract MockQuoter is IQuoter {
    error RevertQuote();

    uint256 public amountOut;
    bool public shouldRevert;

    function setAmountOut(uint256 amountOut_) external {
        amountOut = amountOut_;
    }

    function setShouldRevert(bool shouldRevert_) external {
        shouldRevert = shouldRevert_;
    }

    function quote(address, address, uint256, uint256) external view returns (uint256) {
        if (shouldRevert) revert RevertQuote();
        return amountOut;
    }
}

