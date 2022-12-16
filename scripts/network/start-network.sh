#!/bin/bash

export ROOT=$(pwd)
source $ROOT/scripts/network/vars.sh
cd scripts/network

start_docker() {
    name=$1

    docker compose up -d $name
    docker-compose logs -f $name | sed -r -u "s/\x1B\[([0-9]{1,3}(;[0-9]{1,2})?)?[mGK]//g" > logs/$name.log 2>&1 &

    printf "Waiting for $name to start..."

    ( tail -f -n0 logs/$name.log & ) | grep -q "finalizing commit of block"
    echo "Done"
}

# cleanup any stale state
docker-compose down
rm -rf config logs
mkdir logs

# init chain
bash init-chain.sh $ROOT/build/binary/junod ujuno $ROOT/scripts/network/config/juno
bash init-chain.sh $ROOT/build/binary/osmosisd uosmo $ROOT/scripts/network/config/osmosis

# start docker
start_docker juno
start_docker osmosis

cd ../..