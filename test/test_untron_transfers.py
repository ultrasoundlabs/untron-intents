import pytest
import boa
from boa.test import strategy
from hypothesis import given, settings

@pytest.fixture
def deploy_erc20():
    """
    Deploys a mock ERC20 (from snekmate or your own implementation), returns the Python object bound to it.
    """
    initial_supply = 10**30  # 1 billion tokens
    with boa.env.prank(boa.env.eoa):
        erc20 = boa.load('src/ERC20.vy', "TestToken", "TT", 18, initial_supply)
    return erc20

@pytest.fixture
def deploy_untron_transfers(deploy_erc20):
    """
    Deploys the UntronTransfers contract from your 'src/UntronTransfers.vy', 
    configured with some USDT, USDC addresses, and a trusted relayer.
    """
    mock_trusted_relayer = boa.env.generate_address()
    with boa.env.prank(boa.env.eoa):
        untron = boa.load(
            'src/UntronTransfers.vy',
            deploy_erc20.address,  # _usdt
            deploy_erc20.address,  # _usdc
            mock_trusted_relayer
        )
    return untron, mock_trusted_relayer


def test_deployment_valid_constructor(deploy_untron_transfers, deploy_erc20):
    """
    1. Deployment and Initialization:
       Valid Constructor Arguments
    """
    untron, relayer = deploy_untron_transfers
    # Check that the immutables or storage variables are set correctly
    assert untron.usdt() == deploy_erc20.address
    assert untron.usdc() == deploy_erc20.address
    assert untron.trustedRelayer() == relayer


def test_intron_success(deploy_untron_transfers, deploy_erc20):
    """
    2.1.1 Creating Orders with intron():
       Successful intron()
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()
    input_amount = 1_000
    # Give 'user' some tokens directly:
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, input_amount)

    # Approve the contract to spend 'user' tokens
    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, input_amount)
    
    # Build the order data structure in Python
    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": input_amount,
        "to": b"\x01"*20,   # Some Tron address as bytes20
        "outputAmount": 5000,
        "deadline": boa.env.vm.state.timestamp + 60  # +1 minute from now
    }

    # user calls intron(order)
    nonce_before = untron.nonces(user)
    tx = None
    with boa.env.prank(user):
        tx = untron.intron(order)

    # Check expected state changes
    # 1. The contract's ERC20 balance rises
    assert deploy_erc20.balanceOf(untron.address) == input_amount
    # 2. The order data is stored
    order_id = untron._orderId(order, nonce_before)  # internal function call check
    stored_order = untron.orders(order_id)
    assert stored_order.refundBeneficiary == user
    assert stored_order.inputAmount == input_amount

    # 3. user's nonce increments
    assert untron.nonces(user) == nonce_before + 1

    # 4. Confirm the event was emitted
    logs = tx.logs
    found_event = any(l.event_type == "OrderCreated" for l in logs)
    assert found_event, "OrderCreated event not found"


def test_intron_insufficient_allowance(deploy_untron_transfers, deploy_erc20):
    """
    2.1.2 Failing intron() Because of Insufficient Allowance
    """
    untron, _ = deploy_untron_transfers
    user = boa.env.generate_address()
    input_amount = 1_000

    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, input_amount)

    # No or insufficient approve
    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, 100)  # Less than input_amount

    # Build order
    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": input_amount,
        "to": b"\x01"*20,
        "outputAmount": 5000,
        "deadline": boa.env.vm.state.timestamp + 100
    }

    # Should revert or fail on the token's transferFrom
    with boa.reverts():
        with boa.env.prank(user):
            untron.intron(order)


def test_intron_insufficient_balance(deploy_untron_transfers, deploy_erc20):
    """
    2.1.3 Failing intron() Because of Insufficient Balance
    """
    untron, _ = deploy_untron_transfers
    user = boa.env.generate_address()
    input_amount = 1_000
    # Transfer less than that to user
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, 500)

    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, 1_000)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": input_amount,
        "to": b"\x01"*20,
        "outputAmount": 5000,
        "deadline": boa.env.vm.state.timestamp + 100
    }

    # User doesn't have enough tokens to cover input_amount => revert
    with boa.reverts():
        with boa.env.prank(user):
            untron.intron(order)


def test_intron_zero_input_amount(deploy_untron_transfers, deploy_erc20):
    """
    2.1.4 intron() With Zero Input Amount
    """
    untron, _ = deploy_untron_transfers
    user = boa.env.generate_address()
    # Give user some tokens anyway
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, 1000)

    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, 1000)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": 0,  # The interesting part
        "to": b"\x01"*20,
        "outputAmount": 5000,
        "deadline": boa.env.vm.state.timestamp + 100
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        tx = untron.intron(order)

    # Check no tokens moved:
    assert deploy_erc20.balanceOf(untron.address) == 0
    # Still logs a new order with zero input
    order_id = untron._orderId(order, nonce_before)
    stored_order = untron.orders(order_id)
    assert stored_order.inputAmount == 0


@pytest.mark.ignore_isolation
def test_claim_success(deploy_untron_transfers, deploy_erc20):
    """
    3.1 Successful claim(orderId)
    Demonstrates usage with a timestamp / block progression.
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()

    # 1. The user deposits an order
    amount = 1_000
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, amount)

    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, amount)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": amount,
        "to": b"\x01"*20,
        "outputAmount": 100,
        "deadline": boa.env.vm.state.timestamp + 1000
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        untron.intron(order)
    order_id = untron._orderId(order, nonce_before)

    # 2. The relayer calls claim before deadline
    assert deploy_erc20.balanceOf(untron.address) == amount
    with boa.env.prank(relayer):
        untron.claim(order_id)

    # Check the tokens were transferred to the relayer
    assert deploy_erc20.balanceOf(relayer) == amount
    # The order should be removed from storage
    empty_order = untron.orders(order_id)
    assert empty_order.refundBeneficiary == boa.ZERO_ADDRESS


def test_claim_by_non_relayer(deploy_untron_transfers, deploy_erc20):
    """
    3.2 claim(orderId) Called By Non-Relayer
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()

    # Setup a valid order
    amount = 1_000
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, amount)
    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, amount)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": amount,
        "to": b"\x01"*20,
        "outputAmount": 100,
        "deadline": boa.env.vm.state.timestamp + 1000
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        untron.intron(order)
    order_id = untron._orderId(order, nonce_before)

    # Another random user tries to claim
    attacker = boa.env.generate_address()
    with boa.env.prank(attacker), boa.reverts():
        untron.claim(order_id)


def test_claim_expired_order(deploy_untron_transfers, deploy_erc20):
    """
    3.3 claim(orderId) After Deadline
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()
    amount = 1000

    # Fund user, create order with short deadline
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, amount)

    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, amount)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": amount,
        "to": b"\x01"*20,
        "outputAmount": 100,
        "deadline": boa.env.vm.state.timestamp + 1  # Expires almost immediately
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        untron.intron(order)
    order_id = untron._orderId(order, nonce_before)

    # Time forward beyond the deadline
    boa.env.vm.patch_timestamp(boa.env.vm.state.timestamp + 2)

    with boa.env.prank(relayer), boa.reverts():
        untron.claim(order_id)


def test_cancel_success(deploy_untron_transfers, deploy_erc20):
    """
    4.1 Successful cancel(orderId)
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()
    amt = 500

    # Create an expired order
    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, amt)
    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, amt)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": amt,
        "to": b"\x01"*20,
        "outputAmount": 100,
        "deadline": boa.env.vm.state.timestamp + 1,
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        untron.intron(order)

    order_id = untron._orderId(order, nonce_before)

    # Expire it
    boa.env.vm.patch_timestamp(boa.env.vm.state.timestamp + 2)

    # Cancel
    with boa.env.prank(user):
        untron.cancel(order_id)
    
    # user gets refunded
    assert deploy_erc20.balanceOf(user) == amt
    # order is removed from storage
    empty = untron.orders(order_id)
    assert empty.refundBeneficiary == boa.ZERO_ADDRESS


def test_cancel_before_deadline(deploy_untron_transfers, deploy_erc20):
    """
    4.2 Failing cancel(orderId) Before Deadline
    """
    untron, relayer = deploy_untron_transfers
    user = boa.env.generate_address()

    with boa.env.prank(boa.env.eoa):
        deploy_erc20.transfer(user, 500)
    with boa.env.prank(user):
        deploy_erc20.approve(untron.address, 500)

    order = {
        "refundBeneficiary": user,
        "token": deploy_erc20.address,
        "inputAmount": 500,
        "to": b"\x01"*20,
        "outputAmount": 123,
        "deadline": boa.env.vm.state.timestamp + 1000,
    }

    nonce_before = untron.nonces(user)
    with boa.env.prank(user):
        untron.intron(order)
    order_id = untron._orderId(order, nonce_before)

    # Attempt to cancel early
    with boa.env.prank(user), boa.reverts():
        untron.cancel(order_id)


def test_set_trusted_relayer(deploy_untron_transfers):
    """
    5.1 setTrustedRelayer(newRelayer)
    """
    untron, old_relayer = deploy_untron_transfers
    new_relayer = boa.env.generate_address()

    with boa.env.prank(old_relayer):
        untron.setTrustedRelayer(new_relayer)

    assert untron.trustedRelayer() == new_relayer


def test_set_trusted_relayer_by_non_owner(deploy_untron_transfers):
    """
    5.2 Non-Relayer Trying to setTrustedRelayer()
    """
    untron, old_relayer = deploy_untron_transfers
    random_guy = boa.env.generate_address()
    new_relayer = boa.env.generate_address()

    with boa.env.prank(random_guy), boa.reverts():
        untron.setTrustedRelayer(new_relayer) 