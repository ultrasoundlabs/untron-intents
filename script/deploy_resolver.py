from src import UntronResolver

def deploy():
    resolver = UntronResolver.deploy()
    resolver.setReceiverFactory("0x3B26Cb623edD3F9b1e9e5d4cD60aDc86B48E6D73")
    resolver.pushUrl("https://untron.finance/api/ens/resolve")
    resolver.transfer_ownership("0xf178905915f55dd34Ba1980942354dc64109118F")

    return resolver

def moccasin_main():
    return deploy()