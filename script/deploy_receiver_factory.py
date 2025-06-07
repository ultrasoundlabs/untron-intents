from src import ReceiverFactory

def deploy():
    receiverImplementation = "0xB83176EFAc1D2df0B747F14417a79aF8820ebd48"
    untronTransfers = "0x82aBD2f283529A8Fd95Af96b38664a3Cd1970e80"
    trustedSwapper = "0xa37Cd86db8CE83C842EEAbAFE016aeC920914F25"
    usdt = "0x01bFF41798a0BcF287b996046Ca68b395DbC1071"
    usdc = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"

    receiverFactory = ReceiverFactory.deploy()
    receiverFactory.configure(receiverImplementation, untronTransfers, trustedSwapper, usdt, usdc)
    # receiverFactory.transfer_ownership("0xf178905915f55dd34Ba1980942354dc64109118F")
    
    # resolver = UntronResolver.deploy()
    # resolver.setReceiverFactory(receiverFactory)
    # url = ""
    # resolver.pushUrl(url)

    return receiverFactory

def moccasin_main():
    return deploy()