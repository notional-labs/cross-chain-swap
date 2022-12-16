#!/bin/bash

source scripts/contract-interaction/vars.sh

CONTRACT_ADDRESS=$1
ACCOUNT="test"
SLEEP_TIME="15"
KEYRING="test"

# get channel id
CHANNEL=$(bash scripts/contract-interaction/list-channels.sh "$CONTRACT_ADDRESS")

# from Juno to Osmosis
HOST_ACCOUNT=$(${BINARY[0]} keys show $ACCOUNT -a --keyring-backend $KEYRING --home ${DIR[0]})
DESTINATION_ACCOUNT=$(${BINARY[1]} keys show $ACCOUNT -a --keyring-backend $KEYRING --home ${DIR[1]})

EXECUTE_MESSAGE=$(jq --null-input --arg CHANNEL "$CHANNEL" --arg DESTINATION_ACCOUNT "$DESTINATION_ACCOUNT" '{"transfer":{"channel":$CHANNEL,"remote_address":$DESTINATION_ACCOUNT}}')
echo "$EXECUTE_MESSAGE"
RES=$(${BINARY[0]} tx wasm execute "$CONTRACT_ADDRESS" "$EXECUTE_MESSAGE" --from "$ACCOUNT" --keyring-backend "$KEYRING" --home ${DIR[0]} --chain-id ${CHAINID[0]} --fees 100000${DENOM[0]} --amount 100000000${DENOM[0]} --yes --output json --trace)
echo $RES

if [ "$(echo $RES | jq -r .raw_log)" != "[]" ]; then
    echo "ERROR = $(echo $RES | jq .raw_log)"
    exit 1
else
    echo "EXECUTE SUCCESS"
fi

TXHASH=$(echo $RES | jq -r .txhash)
# sleep for chain to update
sleep "$SLEEP_TIME"

# raw log
RAW_LOG=$(${BINARY[$i]} query tx "$TXHASH" --chain-id "${CHAINID[0]}" --node "${NODE[0]}" -o json | jq -r .raw_log)
echo $RAW_LOG

# check if account balance is updated
BALANCE=$(${BINARY[1]} query bank balances "$DESTINATION_ACCOUNT" --chain-id "${CHAINID[1]}" --node "${NODE[1]}" --output json | jq -r .balances[])
echo $BALANCE