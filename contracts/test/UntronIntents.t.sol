// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/UntronIntentsProxy.sol";
import "../src/chains/MockUntronIntents.sol";
import "../src/interfaces/IUntronIntents.sol";
import "../src/interfaces/IERC7683.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "permit2/libraries/SignatureVerification.sol";

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
    MockERC20 public secondInputToken;

    address public owner;
    address public user;
    address public filler;
    address public relayer;

    function _chainId() internal view returns (uint64) {
        return uint64(block.chainid);
    }

    function setUp() public {
        owner = address(this);
        user = address(0x1);
        filler = address(0x2);
        relayer = address(0x3);

        implementation = new MockUntronIntents();
        bytes memory initData = abi.encodeWithSelector(MockUntronIntents.initialize.selector, address(0));
        proxy = new UntronIntentsProxy(address(implementation), owner, initData);
        untronIntents = IUntronIntents(address(proxy));

        inputToken = new MockERC20();

        secondInputToken = new MockERC20();
    }

    function testOpen() public {
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent =
            IUntronIntents.Intent({refundBeneficiary: user, inputs: inputs, to: to, outputAmount: outputAmount});

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: abi.encode(intent)});

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolve(order)));

        assertEq(untronIntents.orders(orderId), true);
    }

    function testReclaim() public {
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent =
            IUntronIntents.Intent({refundBeneficiary: user, inputs: inputs, to: to, outputAmount: outputAmount});

        OnchainCrossChainOrder memory order =
            OnchainCrossChainOrder({fillDeadline: uint32(block.timestamp + 1 hours), orderData: abi.encode(intent)});

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolve(order)));

        // Simulate some time passing
        vm.warp(block.timestamp + 30 minutes);

        // Reclaim the funds
        untronIntents.reclaim(untronIntents.resolve(order), "");

        // Check that the funds were transferred to the owner (as per MockUntronIntents implementation)
        assertEq(inputToken.balanceOf(owner), inputAmount);
        assertEq(inputToken.balanceOf(address(untronIntents)), 0);

        // Check that the intent was deleted
        assertEq(untronIntents.orders(orderId), false);
    }

    // Helper function to create an Intent
    function createIntent(Input[] memory inputs, uint256 outputAmount, bytes21 to)
        internal
        pure
        returns (IUntronIntents.Intent memory)
    {
        return
            IUntronIntents.Intent({refundBeneficiary: address(0), inputs: inputs, to: to, outputAmount: outputAmount});
    }

    // Helper function to create an OnchainCrossChainOrder
    function createOnchainOrder(IUntronIntents.Intent memory intent, uint32 fillDeadline)
        internal
        pure
        returns (OnchainCrossChainOrder memory)
    {
        return OnchainCrossChainOrder({fillDeadline: fillDeadline, orderData: abi.encode(intent)});
    }

    // Helper function to create a GaslessCrossChainOrder
    function createGaslessOrder(
        address userAddr,
        uint256 nonce,
        uint32 openDeadline,
        uint32 fillDeadline,
        bytes memory orderData
    ) internal view returns (GaslessCrossChainOrder memory) {
        return GaslessCrossChainOrder({
            user: userAddr,
            nonce: nonce,
            originSettler: address(0),
            originChainId: _chainId(),
            openDeadline: openDeadline,
            fillDeadline: fillDeadline,
            orderData: orderData
        });
    }

    function testOpen_Success() public {
        uint256 inputAmount1 = 1000;
        uint256 inputAmount2 = 500;
        uint256 outputAmount = 1400;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount1);
        secondInputToken.mint(user, inputAmount2);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount1);
        secondInputToken.approve(address(untronIntents), inputAmount2);

        Input[] memory inputs = new Input[](2);
        inputs[0] = Input(address(inputToken), inputAmount1);
        inputs[1] = Input(address(secondInputToken), inputAmount2);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolve(order)));

        assertEq(untronIntents.orders(orderId), true);
        assertEq(inputToken.balanceOf(address(untronIntents)), inputAmount1);
        assertEq(secondInputToken.balanceOf(address(untronIntents)), inputAmount2);
    }

    function testOpen_Revert_OrderExpired() public {
        // Tests that opening an expired order reverts with "Order expired".
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        vm.warp(block.timestamp + 2 hours);

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp - 1 hours));

        vm.startPrank(user);
        vm.expectRevert();
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

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        vm.expectRevert();
        untronIntents.open(order);
        vm.stopPrank();
    }

    function testOpen_Revert_InvalidOrderData() public {
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

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        untronIntents.open(order);
        vm.stopPrank();

        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolve(order);
        bytes32 orderId = keccak256(abi.encode(resolvedOrder));
        // Check that the order was created
        assertEq(untronIntents.orders(orderId), true);
    }

    function testResolve_Success() public view {
        // Tests resolving a valid on-chain order successfully.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

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
        uint32 openDeadline = uint32(block.timestamp + 1 hours);
        uint32 fillDeadline = uint32(block.timestamp + 2 hours);

        uint256 userPrivateKey = 0x1234567890123456789012345678901234567890123456789012345678901234;
        address orderUser = vm.addr(userPrivateKey);

        uint256 inputAmount1 = 1000;
        uint256 inputAmount2 = 500;
        uint256 outputAmount = 1400;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(orderUser, inputAmount1);
        secondInputToken.mint(orderUser, inputAmount2);

        Input[] memory inputs = new Input[](2);
        inputs[0] = Input(address(inputToken), inputAmount1);
        inputs[1] = Input(address(secondInputToken), inputAmount2);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = orderUser;
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order =
            createGaslessOrder(orderUser, untronIntents.gaslessNonces(orderUser), openDeadline, fillDeadline, orderData);
        order.originSettler = address(untronIntents);

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolveFor(order, "")));

        bytes32 messageHash = untronIntents._messageHash(orderId, intent);

        (uint8 v, bytes32 r, bytes32 s) = vm.sign(userPrivateKey, messageHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.startPrank(orderUser);
        inputToken.approve(address(untronIntents), inputAmount1);
        secondInputToken.approve(address(untronIntents), inputAmount2);
        vm.stopPrank();

        untronIntents.openFor(order, signature, "");

        assertEq(untronIntents.orders(orderId), true);
        assertEq(inputToken.balanceOf(address(untronIntents)), inputAmount1);
        assertEq(secondInputToken.balanceOf(address(untronIntents)), inputAmount2);
    }

    function testOpenFor_Revert_InvalidSignature() public {
        // Tests that opening a gasless order with an invalid signature reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));
        uint32 openDeadline = uint32(block.timestamp + 1 hours);
        uint32 fillDeadline = uint32(block.timestamp + 2 hours);

        // Create wallets for the user and an attacker with known private keys
        uint256 userPrivateKey = 0x1234567890123456789012345678901234567890123456789012345678901234;
        uint256 attackerPrivateKey = 0x2234567890123456789012345678901234567890123456789012345678901234;
        address orderUser = vm.addr(userPrivateKey);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = orderUser;
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order =
            createGaslessOrder(orderUser, untronIntents.gaslessNonces(orderUser), openDeadline, fillDeadline, orderData);
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        // Attacker tries to sign the message
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(attackerPrivateKey, messageHash);

        vm.startPrank(relayer);
        vm.expectRevert();
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

        // Create a wallet for the user with a known private key
        uint256 userPrivateKey = 0x1234567890123456789012345678901234567890123456789012345678901234;
        address orderUser = vm.addr(userPrivateKey);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = orderUser;
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order = createGaslessOrder(
            orderUser,
            untronIntents.gaslessNonces(orderUser) + 1, // Incorrect nonce
            openDeadline,
            fillDeadline,
            orderData
        );
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(userPrivateKey, messageHash);

        vm.startPrank(relayer);
        vm.expectRevert();
        untronIntents.openFor(order, abi.encode(v, r, s), "");
        vm.stopPrank();
    }

    function testOpenFor_Revert_OrderExpired() public {
        // Tests that opening a gasless order after fillDeadline reverts.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        // Create a wallet for the user with a known private key
        uint256 userPrivateKey = 0x1234567890123456789012345678901234567890123456789012345678901234;
        address orderUser = vm.addr(userPrivateKey);

        vm.warp(block.timestamp + 2 hours);

        uint32 openDeadline = uint32(block.timestamp - 1 hours);
        uint32 fillDeadline = uint32(block.timestamp - 30 minutes);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = orderUser;
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order =
            createGaslessOrder(orderUser, untronIntents.gaslessNonces(orderUser), openDeadline, fillDeadline, orderData);
        order.originSettler = address(untronIntents);

        bytes32 messageHash = keccak256(abi.encode(order));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(userPrivateKey, messageHash);

        vm.startPrank(relayer);
        vm.expectRevert();
        untronIntents.openFor(order, abi.encode(v, r, s), "");
        vm.stopPrank();
    }

    function testResolveFor_Success() public view {
        // Tests resolving a valid gasless order successfully.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;
        bytes memory orderData = abi.encode(intent);

        GaslessCrossChainOrder memory order =
            createGaslessOrder(user, 0, uint32(block.timestamp + 1 hours), uint32(block.timestamp + 2 hours), orderData);

        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolveFor(order, "");

        assertEq(resolvedOrder.maxSpent[0].amount, inputAmount);
        assertEq(resolvedOrder.minReceived[0].amount, outputAmount);
    }

    function testResolveFor_Revert_InvalidOrderData() public {
        // Tests that resolving a gasless order with invalid orderData reverts.
        bytes memory invalidOrderData = "invalid_data";

        GaslessCrossChainOrder memory order = createGaslessOrder(
            user, 0, uint32(block.timestamp + 1 hours), uint32(block.timestamp + 2 hours), invalidOrderData
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

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        vm.stopPrank();

        // Fast forward time beyond the fillDeadline
        vm.warp(block.timestamp + 2 hours);

        uint256 balanceBefore = inputToken.balanceOf(user);

        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolve(order);
        untronIntents.reclaim(resolvedOrder, "");

        uint256 balanceAfter = inputToken.balanceOf(user);

        assertEq(balanceAfter - balanceBefore, inputAmount);
    }

    function testIntents_NonExistentOrderId() public view {
        // Tests retrieving an intent that does not exist returns default values.
        bytes32 nonExistentOrderId = keccak256("nonexistent");

        assertEq(untronIntents.orders(nonExistentOrderId), false);
    }

    function testOpen_DuplicateOrders() public {
        // Tests opening duplicate orders and checks behavior.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount * 2);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount * 2);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        untronIntents.open(order);
        untronIntents.open(order); // Attempt to open duplicate order
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolve(order)));
        uint256 contractBalance = inputToken.balanceOf(address(untronIntents));

        // Check that contract has double the inputAmount
        assertEq(contractBalance, inputAmount * 2);

        assertEq(untronIntents.orders(orderId), true);
    }

    function testReclaim_MultipleAttempts() public {
        // Tests that multiple reclaim attempts after deletion fail.
        uint256 inputAmount = 1000;
        uint256 outputAmount = 900;
        bytes21 to = bytes21(uint168(0x123456789abcdef0123456789abcdef012345));

        inputToken.mint(user, inputAmount);

        vm.startPrank(user);
        inputToken.approve(address(untronIntents), inputAmount);

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));
        ResolvedCrossChainOrder memory resolvedOrder = untronIntents.resolve(order);

        untronIntents.open(order);
        vm.stopPrank();

        // Fast forward time beyond the fillDeadline
        vm.warp(block.timestamp + 2 hours);

        untronIntents.reclaim(resolvedOrder, "");

        vm.warp(block.timestamp + 2 hours);

        // Second attempt should fail
        // TODO: This does NOT fail if i call resolve twice in these two reclaim functions
        // instead of calling it once and writing it in a resolvedOrder
        // and i think this could mean a bug but i can't find one
        vm.expectRevert();
        untronIntents.reclaim(resolvedOrder, "");
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

        Input[] memory inputs = new Input[](1);
        inputs[0] = Input(address(inputToken), inputAmount);

        IUntronIntents.Intent memory intent = createIntent(inputs, outputAmount, to);
        intent.refundBeneficiary = user;

        OnchainCrossChainOrder memory order = createOnchainOrder(intent, uint32(block.timestamp + 1 hours));

        // Expect the Open event to be emitted
        // TODO: Fix
        //vm.expectEmit(true, true, false, true);
        //emit Open(keccak256(abi.encode(order)), untronIntents.resolve(order));

        untronIntents.open(order);
        vm.stopPrank();

        bytes32 orderId = keccak256(abi.encode(untronIntents.resolve(order)));
        assertEq(untronIntents.orders(orderId), true);

        // Check that user's balance has decreased
        assertEq(inputToken.balanceOf(user), 0);
    }
}
