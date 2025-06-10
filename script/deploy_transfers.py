from src import UntronTransfers

def moccasin_main():
    usdt = "0x01bFF41798a0BcF287b996046Ca68b395DbC1071"
    usdc = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"
    contract = UntronTransfers.deploy(usdt, usdc)
    # contract.configure(
    #     newRelayer="0xa37Cd86db8CE83C842EEAbAFE016aeC920914F25",
    #     fixedFee=2000000,
    #     percentFee=0,
    #     referrerFee=500000
    # )
    contract.transfer_ownership(
        "0xf178905915f55dd34Ba1980942354dc64109118F"
    )
    return contract


if __name__ == "__main__":
    moccasin_main()