#!/bin/bash

# build cargo
cargo build

# detect architecture and decide image
arch=$(uname -m)
rust_optimizer_image="cosmwasm/rust-optimizer:0.12.10"
if [ $arch == "arm64" ]; then
  # cosmwasm/rust-optimizer-arm64:0.12.10 is not yet on docker hub, manual build is required
  rust_optimizer_image="cosmwasm/rust-optimizer-arm64:0.12.10"

  if ! docker image inspect $rust_optimizer_image &>/dev/null; then
    mkdir build
    wget -c https://github.com/CosmWasm/rust-optimizer/archive/refs/tags/v0.12.10.zip -O build/v0.12.10.zip
    unzip build/v0.12.10.zip -d build
    cd build/rust-optimizer-0.12.10
    make build-rust-optimizer-arm64
    cd ../..
  fi
fi

# compile contract
sudo docker run --rm -v "$(pwd)":/code \
  --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target \
  --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
  $rust_optimizer_image