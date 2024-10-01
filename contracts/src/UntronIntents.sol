// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "./interfaces/IERC7683.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

abstract contract UntronIntents is ISettlementContract {
    mapping(address => uint256) public nonces;

    struct Intent {
        address sourceToken;
        uint256 amount;
        bytes32 to;
        uint256 minOutput;
    }

    function initiate(CrossChainOrder calldata order, bytes calldata signature, bytes calldata fillerData)
        external
        override
    {
        require(order.settlementContract == address(this), "Wrong contract");

        if (order.swapper != msg.sender) {
            (bytes32 r, bytes32 s, uint8 v) = abi.decode(signature, (bytes32, bytes32, uint8));
            address recoveredAddress = ecrecover(keccak256(abi.encode(order)), v, r, s);
            require(recoveredAddress == order.swapper, "Invalid signature");
        }

        require(order.nonce == nonces[order.swapper], "Invalid nonce");
        nonces[order.swapper]++;

        uint256 chainId;
        assembly {
            chainId := chainid()
        }
        require(order.originChainId == chainId, "Wrong chain");

        require(order.initiateDeadline < block.timestamp, "Order expired");
        require(order.fillDeadline > block.timestamp, "Order expired");

        Intent memory intent = abi.decode(order.orderData, (Intent));
    }
}
