#!/bin/bash

# Juno, Osmosis
export BINARY=( "build/binary/junod" "build/binary/osmosisd" )
export DIR=( "scripts/network/config/juno" "scripts/network/config/osmosis" )
export DENOM=( "ujuno" "uosmo" )
export NODE=( "http://localhost:26657" "http://localhost:26357" )
export CHAINID=( "test-juno" "test-osmo" )