#!/bin/bash

bash scripts/network/build-network.sh
bash scripts/network/start-network.sh

bash scripts/build_contract.sh
bash scripts/deploy_contract.sh

# check out contract-interaction folder to see a list of scripts that can be used to interact with the contract
# get juno contract addresses written in scripts/contract-interaction/logs/contract-addresses.txt
# replace juno14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9skjuwg8
# "bash scripts/contract-interaction/transfer-cross-chain.sh juno14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9skjuwg8"