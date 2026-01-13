// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

contract MockReverter {
    function boom() external pure {
        revert("boom");
    }
}

