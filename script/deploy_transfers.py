from moccasin.config import get_config
from src import UntronTransfers

def moccasin_main():
    active_network = get_config().get_active_network()
    usdt = active_network.get_named_contract("usdt")
    usdc = active_network.get_named_contract("usdc")
    contract = UntronTransfers.deploy(usdt.address, usdc.address)
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