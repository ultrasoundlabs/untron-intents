// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/UntronIntentsProxy.sol";
import "../src/chains/MockUntronIntents.sol";
import "../src/interfaces/IUntronIntents.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract MockERC20 is ERC20 {
    constructor() ERC20("Mock ERC20", "MC20") {}

    function mint(address to, uint256 amount) public {
        _mint(to, amount);
    }
}

contract UntronIntentsTest is Test {
    UntronIntentsProxy public proxy;
    MockUntronIntents public implementation;
    IUntronIntents public untronIntents;
    MockERC20 public inputToken;

    address public owner;
    address public user;
    address public filler;

    function setUp() public {
        owner = address(this);
        user = address(0x1);
        filler = address(0x2);

        implementation = new MockUntronIntents();
        bytes memory initData = abi.encodeWithSelector(MockUntronIntents.initialize.selector);
        proxy = new UntronIntentsProxy(address(implementation), owner, initData);
        untronIntents = IUntronIntents(address(proxy));

        inputToken = new MockERC20();
    }

    function testOpen() public {
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = IUntronIntents.Intent({
            user: user,
            inputToken: address(inputToken),
            inputAmount: inputAmount,
            to: to,
            outputAmount: outputAmount
        });

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: abi.encode(intent)});

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));
        IUntronIntents.Intent memory storedIntent = untronIntents.intents(orderId);

        assertEq(storedIntent.user, user);
        assertEq(storedIntent.inputToken, address(inputToken));
        assertEq(storedIntent.inputAmount, inputAmount);
        assertEq(storedIntent.to, to);
        assertEq(storedIntent.outputAmount, outputAmount);
        assertEq(inputToken.balanceOf(address(untronIntents)), inputAmount);
    }

    function testReclaim() public {
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = IUntronIntents.Intent({
            user: user,
            inputToken: address(inputToken),
            inputAmount: inputAmount,
            to: to,
            outputAmount: outputAmount
        });

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: abi.encode(intent)});

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));

        // Simulate some time passing
        vm.warp(block.timestamp + 2 hours);

        // Reclaim the funds
        untronIntents.reclaim(orderId, "");

        // Check that the funds were transferred to the owner (as per MockUntronIntents implementation)
        assertEq(inputToken.balanceOf(owner), inputAmount);
        assertEq(inputToken.balanceOf(address(untronIntents)), 0);

        // Check that the intent was deleted
        IUntronIntents.Intent memory storedIntent = untronIntents.intents(orderId);
        assertEq(storedIntent.user, address(0));
        assertEq(storedIntent.inputToken, address(0));
        assertEq(storedIntent.inputAmount, 0);
        assertEq(storedIntent.to, bytes21(0));
        assertEq(storedIntent.outputAmount, 0);
    }
}
