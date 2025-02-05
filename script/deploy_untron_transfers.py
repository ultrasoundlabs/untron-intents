from src import UntronTransfers

def moccasin_main():
    usdt = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"
    usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    return UntronTransfers.deploy(usdt, usdc)