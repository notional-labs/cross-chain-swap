#!/bin/bash

SRC_ADDRESS=$1
DST_ADDRESS=$2

# ROOT=$(pwd)

docker container stop sc-relayer &>/dev/null
docker container rm sc-relayer &>/dev/null
rm -rf $ROOT/scripts/network/config/relayer-sc-config $ROOT/scripts/network/logs/relayer-sc.log

source $ROOT/scripts/network/vars.sh
relayer_config=$ROOT/scripts/network/config/relayer-sc-config/config
relayer_logs=$ROOT/scripts/network/logs/relayer-sc.log
relayer_exec="docker-compose -f $ROOT/scripts/network/docker-compose.yml run --rm sc-relayer"

mkdir -p $relayer_config
# modify relayer-sc-config.yaml to reflect the correct contract address
cp scripts/network/relayer-sc-config.yaml $relayer_config/config.yaml

$relayer_exec rly keys restore juno rly-juno "$MNEMONIC_3" >> $relayer_logs 2>&1
$relayer_exec rly keys restore osmosis rly-osmo "$MNEMONIC_3" >> $relayer_logs 2>&1

printf "Waiting for relayer to start..."
$relayer_exec rly transact link sc_juno-osmosis --src-port $SRC_ADDRESS --dst-port $DST_ADDRESS --version ics20-1 >> $relayer_logs 2>&1

docker-compose -f $ROOT/scripts/network/docker-compose.yml up -d sc-relayer
docker-compose -f $ROOT/scripts/network/docker-compose.yml logs -f sc-relayer | sed -r -u "s/\x1B\[([0-9]{1,3}(;[0-9]{1,2})?)?[mGK]//g" >> $relayer_logs 2>&1 &

echo "Done"