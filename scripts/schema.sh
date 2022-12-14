for d in contracts/*; do
  if [ -d "$d" ]; then
    cd $d
    cargo build
    cargo schema
    cd ../..
  fi
done
