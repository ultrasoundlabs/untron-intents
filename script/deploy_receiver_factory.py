from src import ReceiverFactory

def deploy():
    receiver = "0xF455a36B6a937e78844a9bB5D0E7C021EEBFfFBA"
    untronTransfers = "0x8d2Db6153188b002fc2E662538948Be3C5aE65F7"
    trustedSwapper = "0xa37Cd86db8CE83C842EEAbAFE016aeC920914F25"
    usdt = "0x01bFF41798a0BcF287b996046Ca68b395DbC1071"
    usdc = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"

    receiverFactory = ReceiverFactory.deploy()
    receiverFactory.configure(receiver, untronTransfers, trustedSwapper, usdt, usdc)
    # receiverFactory.transfer_ownership("0xf178905915f55dd34Ba1980942354dc64109118F")
    
    # resolver = UntronResolver.deploy()
    # resolver.setReceiverFactory(receiverFactory)
    # url = ""
    # resolver.pushUrl(url)

    return receiverFactory

def moccasin_main():
    return deploy()