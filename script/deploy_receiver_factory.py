from src import ReceiverFactory
from src import UntronReceiver

def deploy():
    usdt = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"
    usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    # flexSwapper = "0x0000000000000000000000000000000000000000" # fake address
    untronTransfers = "0x8d2Db6153188b002fc2E662538948Be3C5aE65F7"

    receiver = UntronReceiver.deploy()

    receiverFactory = ReceiverFactory.deploy()
    receiverFactory.setReceiverImplementation(receiver)
    # receiverFactory.setFlexSwapper(flexSwapper)
    receiverFactory.setUntronTransfers(untronTransfers)
    receiverFactory.setUsdt(usdt)
    receiverFactory.setUsdc(usdc)
    receiverFactory.transfer_ownership("0xf178905915f55dd34Ba1980942354dc64109118F")
    
    # resolver = UntronResolver.deploy()
    # resolver.setReceiverFactory(receiverFactory)
    # url = ""
    # resolver.pushUrl(url)

    return receiverFactory

def moccasin_main():
    return deploy()