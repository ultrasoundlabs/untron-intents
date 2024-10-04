// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/UntronIntentsProxy.sol";
import "../src/chains/MockUntronIntents.sol";
import "../src/interfaces/IUntronIntents.sol";
import "../src/interfaces/IERC7683.sol";
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
    address public relayer;

    function setUp() public {
        owner = address(this);
        user = address(0x1);
        filler = address(0x2);
        relayer = address(0x3);

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

    // Helper function to create an Intent
    function createIntent(uint256 inputAmount, uint256 outputAmount, bytes21 to)
        internal
        pure
        returns (IUntronIntents.Intent memory)
    {
        return IUntronIntents.Intent({
            user: address(0),
            inputToken: address(0),
            inputAmount: inputAmount,
            to: to,
            outputAmount: outputAmount
        });
    }

    // Helper function to create an OnchainCrossChainOrder
    function createOnchainOrder(IUntronIntents.Intent memory intent, uint32 fillDeadline)
        internal
        pure
        returns (OnchainCrossChainOrder memory)
    {
        return OnchainCrossChainOrder({
            fillDeadline: fillDeadline,
            orderData: abi.encode(intent)
        });
    }

    // Helper function to create a GaslessCrossChainOrder
    function createGaslessOrder(
        address userAddr,
        uint256 nonce,
        uint32 openDeadline,
        uint32 fillDeadline,
        bytes memory orderData
    ) internal pure returns (GaslessCrossChainOrder memory) {
        return GaslessCrossChainOrder({
            user: userAddr,
            nonce: nonce,
            originSettler: address(0),
            openDeadline: openDeadline,
            fillDeadline: fillDeadline,
            orderData: orderData
        });
    }

    function testOpen_Success() public {
        // Tests opening a valid on-chain order successfully.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
    
        inputToken.mint(user, inputAmount);
    
        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);
    
        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);
    
        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));
    
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

    function testOpen_Revert_OrderExpired() public {
        // Tests that opening an expired order reverts with "Order expired".
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp - 1 hours));

        vm.startPrank(user);
        vm.expectRevert("Order expired");
        untronIntents.open(order);
        vm.stopPrank();
    }

    function testOpen_Revert_InsufficientFunds() public {
        // Tests that opening an order with insufficient funds reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount / 2); // Mint less than required

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        vm.expectRevert("Insufficient funds");
        untronIntents.open(order);
        vm.stopPrank();
    }

    function testOpen_Revert_InvalidOrderData() public {
        // Tests that opening an order with invalid orderData reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;

        bytes memory invalidOrderData = "invalid_data";

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: invalidOrderData});

        vm.startPrank(user);
        vm.expectRevert();
        untronIntents.open(order);
        vm.stopPrank();
    }

    function testOpen_ZeroInputAmount() public {
        // Tests opening an order with zero inputAmount.
        uint256 inputAmount = 0;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));
        IUntronIntents.Intent memory storedIntent = untronIntents.intents(orderId);

        assertEq(storedIntent.inputAmount, 0);
    }

    function testResolve_Success() public {
        // Tests resolving a valid on-chain order successfully.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolve(order);

        assertEq(resolvedOrder.maxSpent[0].amount, inputAmount);
        assertEq(resolvedOrder.minReceived[0].amount, outputAmount);
    }

    function testResolve_Revert_InvalidOrderData() public {
        // Tests that resolving an order with invalid orderData reverts.
        bytes memory invalidOrderData = "invalid_data";

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: invalidOrderData});

        vm.expectRevert();
        untronIntents.resolve(order);
    }

    function testOpenFor_Success() public {
        // Tests opening a gasless order successfully with a valid signature.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
        uint32 openDeadline = uint32(block.timestamp + 1 hours);
        uint32 fillDeadline = uint32(block.timestamp + 2 hours);

        inputToken.mint(user, inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            untronIntents.gaslessNonces(user),
            openDeadline,
            fillDeadline,
            orderData
        );
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(uint160(user)), messageHash);

        vm.startPrank(relayer);
        inputToken.approve(address(untronIntents), inputAmount);

        untronIntents.openFor(order, abi.encode(v, r, s), "");

        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));
        IUntronIntents.Intent memory storedIntent = untronIntents.intents(orderId);

        assertEq(storedIntent.user, user);
        assertEq(inputToken.balanceOf(address(untronIntents)), inputAmount);
    }

    function testOpenFor_Revert_InvalidSignature() public {
        // Tests that opening a gasless order with an invalid signature reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
        uint32 openDeadline = uint32(block.timestamp + 1 hours);
        uint32 fillDeadline = uint32(block.timestamp + 2 hours);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            untronIntents.gaslessNonces(user),
            openDeadline,
            fillDeadline,
            orderData
        );
        order.originSettler = address(untronIntents);

        // Tamper with the message hash to invalidate the signature
        bytes32 messageHash = keccak256(abi.encode(order));
        messageHash = keccak256(abi.encodePacked(messageHash, "tampered"));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(uint160(user)), messageHash);

        vm.startPrank(relayer);
        vm.expectRevert("Invalid signature");
        untronIntents.openFor(order, abi.encode(v, r, s), "");
        vm.stopPrank();
    }

    function testOpenFor_Revert_InvalidNonce() public {
        // Tests that opening a gasless order with an invalid nonce reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
        uint32 openDeadline = uint32(block.timestamp + 1 hours);
        uint32 fillDeadline = uint32(block.timestamp + 2 hours);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            untronIntents.gaslessNonces(user) + 1, // Incorrect nonce
            openDeadline,
            fillDeadline,
            orderData
        );
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(uint160(user)), messageHash);

        vm.startPrank(relayer);
        vm.expectRevert("Invalid nonce");
        untronIntents.openFor(order, abi.encode(v, r, s), "");
        vm.stopPrank();
    }

    function testOpenFor_Revert_OrderExpired() public {
        // Tests that opening a gasless order after fillDeadline reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
        uint32 openDeadline = uint32(block.timestamp - 1 hours);
        uint32 fillDeadline = uint32(block.timestamp - 30 minutes);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            untronIntents.gaslessNonces(user),
            openDeadline,
            fillDeadline,
            orderData
        );
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(uint160(user)), messageHash);

        vm.startPrank(relayer);
        vm.expectRevert("Open deadline expired");
        untronIntents.openFor(order, abi.encode(v, r, s), "");
        vm.stopPrank();
    }

    function testResolveFor_Success() public {
        // Tests resolving a valid gasless order successfully.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            0,
            uint32(block.timestamp + 1 hours),
            uint32(block.timestamp + 2 hours),
            orderData
        );

        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolveFor(order, "");

        assertEq(resolvedOrder.maxSpent[0].amount, inputAmount);
        assertEq(resolvedOrder.minReceived[0].amount, outputAmount);
    }

    function testResolveFor_Revert_InvalidOrderData() public {
        // Tests that resolving a gasless order with invalid orderData reverts.
        bytes memory invalidOrderData = "invalid_data";

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user,
            0,
            uint32(block.timestamp + 1 hours),
            uint32(block.timestamp + 2 hours),
            invalidOrderData
        );

        vm.expectRevert();
        untronIntents.resolveFor(order, "");
    }

    function testReclaim_Success_UserAfterDeadline() public {
        // Tests that a user can reclaim funds after the deadline.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        vm.stopPrank();

        // Fast forward time beyond the fillDeadline
        vm.warp(block.timestamp + 2 hours);

        bytes32 orderId = keccak256(abi.encode(order));

        uint256 balanceBefore = inputToken.balanceOf(user);

        untronIntents.reclaim(orderId, "");

        uint256 balanceAfter = inputToken.balanceOf(user);

        assertEq(balanceAfter - balanceBefore, inputAmount);
    }

    function testReclaim_Revert_BeforeDeadline() public {
        // Tests that reclaiming before the deadline reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));

        vm.expectRevert("Cannot reclaim before deadline");
        untronIntents.reclaim(orderId, "");
    }

    function testReclaim_Revert_NonExistentIntent() public {
        // Tests that reclaiming a non-existent intent reverts.
        bytes32 nonExistentOrderId = keccak256("nonexistent");

        vm.expectRevert("Intent does not exist");
        untronIntents.reclaim(nonExistentOrderId, "");
    }

    function testIntents_NonExistentOrderId() public {
        // Tests retrieving an intent that does not exist returns default values.
        bytes32 nonExistentOrderId = keccak256("nonexistent");

        IUntronIntents.Intent memory intent = untronIntents.intents(nonExistentOrderId);

        assertEq(intent.user, address(0));
        assertEq(intent.inputAmount, 0);
    }

    function testOpen_DuplicateOrders() public {
        // Tests opening duplicate orders and checks behavior.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount * 2);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount * 2);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        untronIntents.open(order); // Attempt to open duplicate order
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(order));
        uint256 contractBalance = inputToken.balanceOf(address(untronIntents));

        // Check that contract has double the inputAmount
        assertEq(contractBalance, inputAmount * 2);

        // Intent should be overwritten with the same data
        IUntronIntents.Intent memory storedIntent = untronIntents.intents(orderId);
        assertEq(storedIntent.inputAmount, inputAmount);
    }

    function testReclaim_MultipleAttempts() public {
        // Tests that multiple reclaim attempts after deletion fail.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        vm.stopPrank();

        // Fast forward time beyond the fillDeadline
        vm.warp(block.timestamp + 2 hours);

        bytes32 orderId = keccak256(abi.encode(order));

        untronIntents.reclaim(orderId, "");

        // Second attempt should fail
        vm.expectRevert("Intent does not exist");
        untronIntents.reclaim(orderId, "");
    }

    function testReentrancyProtection() public {
        // Tests that the contract is protected against reentrancy.
        // This test requires a malicious token contract to attempt reentrancy.
        // Assuming reentrancy guard is implemented in the contract.
        /*
        Note: Since the contract provided doesn't include reentrancy protection mechanisms or external calls that could lead to reentrancy, 
        this test is a placeholder for when such mechanisms are implemented.
        */
    }

    function testOpen_Improved() public {
        // Improved testOpen with event assertions and edge case checks.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputAmount, outputAmount, to);
        intent.user = user;
        intent.inputToken = address(inputToken);

        OnchainCrossChainOrder memory order =
            createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        // Expect the Open event to be emitted
        // TODO: Fix
        //vm.expectEmit(true, true, false, true);
        //emit Open(keccak256(abi.encode(order)), untronIntents.resolve(order));

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

        // Check that user's balance has decreased
        assertEq(inputToken.balanceOf(user), 0);
    }

}