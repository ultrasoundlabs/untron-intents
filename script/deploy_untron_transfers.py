from src import UntronTransfers

def moccasin_main():
    usdt = "0x8d2Db6153188b002fc2E662538948Be3C5aE65F7"
    usdc = "0x8d2Db6153188b002fc2E662538948Be3C5aE65F7"
    return UntronTransfers.deploy(usdt, usdc)