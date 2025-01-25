from src import UntronTransfers

def moccasin_main():
    usdt = "0xd07308A887ffA74b8965C0F26e6E2e70072C97b9"
    usdc = "0xd07308A887ffA74b8965C0F26e6E2e70072C97b9"
    return UntronTransfers.deploy(usdt, usdc)