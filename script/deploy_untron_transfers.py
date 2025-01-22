from src import UntronTransfers

def moccasin_main():
    usdt = "0x9Ab408d64CB0Ed1fE08B4D2CC0a4f36c43dA1FB1"
    usdc = "0x9Ab408d64CB0Ed1fE08B4D2CC0a4f36c43dA1FB1"
    return UntronTransfers.deploy(usdt, usdc)