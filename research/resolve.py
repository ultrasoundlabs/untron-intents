from web3 import Web3
from ens import ENS

w3 = Web3(Web3.HTTPProvider('https://rpc.ankr.com/eth'))
ns = ENS.from_web3(w3)

print("Resolving TDWrw2Ra3tBCQjWwzFf387Z57bLrYq7YTr.trc.eth")
resolved = ns.address("TDWrw2Ra3tBCQjWwzFf387Z57bLrYq7YTr.trc.eth")
print("Resolved to", resolved)