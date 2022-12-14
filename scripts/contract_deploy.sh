#!/bin/bash

ACCOUNT="test"
SLEEP_TIME="15"
KEYRING="test"

# JUNO, OSMOSIS
NODE=( "http://localhost:26657" "http://localhost:26357" )
CHAINID=( "test-juno" "test-osmo" )
BINARY=( "build/binary/junod" "build/binary/osmosisd" )
DIR=( "scripts/network/config/juno" "scripts/network/config/osmosis" )
DENOM=( "ujuno" "uosmo" )

CONTRACT_DIR=( "" "artifacts/cw_ics20_swap.wasm" )
arch=$(uname -m)
if [ $arch == "arm64" ]; then
    CONTRACT_DIR=( "" "artifacts/cw_ics20_swap-aarch64.wasm" )
fi
INIT_MSG=( '' '{"default_timeout" : 60}' )

# loop contracts
for j in {0..1}; do
    if [ "${CONTRACT_DIR[$j]}" = "" ]; then
        continue
    fi
    echo "DEPLOYING ${CONTRACT_DIR[$j]}"
    # loop chains
    for i in {0..1}; do
        # store contract
        RES=$(${BINARY[$i]} tx wasm store "${CONTRACT_DIR[$j]}" --from "$ACCOUNT" -y --output json --chain-id "${CHAINID[$i]}" --node "${NODE[$i]}" --gas 20000000 --fees 875000${DENOM[$i]} -y --output json --keyring-backend $KEYRING --home ${DIR[$i]})
        echo $RES

        if [ "$(echo $RES | jq -r .raw_log)" != "[]" ]; then
            # exit
            echo "ERROR = $(echo $RES | jq .raw_log)"
            exit 1
        else
            echo "STORE SUCCESS"
        fi

        TXHASH=$(echo $RES | jq -r .txhash)

        echo $TXHASH

        # sleep for chain to update
        sleep "$SLEEP_TIME"

        # query code id
        RAW_LOG=$(${BINARY[$i]} query tx "$TXHASH" --chain-id "${CHAINID[$i]}" --node "${NODE[$i]}" -o json | jq -r .raw_log)

        echo $RAW_LOG

        CODE_ID=$(echo $RAW_LOG | jq -r .[0].events[1].attributes[1].value)

        echo "CODE_ID on ${CHAINID[$i]} = $CODE_ID"

        # instantiate contract
        if [ "${INIT_MSG[$j]}" = "" ]; then
            continue
        fi

        RES=$(${BINARY[$i]} tx wasm instantiate "$CODE_ID" "${INIT_MSG[$j]}" --from "$ACCOUNT" --no-admin --label "contract" -y --chain-id "${CHAINID[$i]}" --node "${NODE[$i]}" --gas 20000000 --fees 100000${DENOM[$i]} -o json --keyring-backend $KEYRING --home ${DIR[$i]})

        if [ "$(echo $RES | jq -r .raw_log)" != "[]" ]; then
            # exit
            echo "ERROR = $(echo $RES | jq .raw_log)"
            exit 1
        else
            echo "INSTANTIATE SUCCESS"
        fi

        # sleep for chain to update
        sleep "$SLEEP_TIME"

        # query contract address
        RAW_LOG=$(${BINARY[$i]} query tx "$(echo $RES | jq -r .txhash)" --chain-id "${CHAINID[$i]}" --node "${NODE[$i]}" -o json | jq -r .raw_log)
        echo $RAW_LOG
        CONTRACT_ADDRESS=$(echo $RAW_LOG | jq -r .[0].events[0].attributes[0].value)
        echo "CONTRACT ADDRESS on ${CHAINID[$i]} = $CONTRACT_ADDRESS"
    done
    echo "DONE DEPLOYING ${CONTRACT_DIR[$j]}"
    echo
    echo
    echo
done
