from src import UntronTransfers

def moccasin_main():
    usdt = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"
    usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    contract = UntronTransfers.deploy(usdt, usdc)
    contract.setRecommendedFee(500000, 0)
    return contract


if __name__ == "__main__":
    moccasin_main()
