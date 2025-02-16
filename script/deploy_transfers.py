from src import UntronTransfers

def moccasin_main():
    usdt = "0xd07308A887ffA74b8965C0F26e6E2e70072C97b9"
    usdc = "0xd07308A887ffA74b8965C0F26e6E2e70072C97b9"
    contract = UntronTransfers.deploy(usdt, usdc)
    contract.setRecommendedFee(500000, 0)
    return contract


if __name__ == "__main__":
    moccasin_main()