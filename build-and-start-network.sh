#!/bin/bash

bash scripts/network/build-network.sh
bash scripts/network/start-network.sh

bash scripts/build_contract.sh
bash scripts/deploy_contract.sh

# check out contract-interaction folder to see a list of scripts that can be used to interact with the contract
# contract addresses are written in scripts/contract-interaction/logs/contract-addresses.txt