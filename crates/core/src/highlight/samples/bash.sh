#!/bin/sh
# Builds CPython in a folder of its own.
set -e
build="${1:-build}"
mkdir -p "$build" && cd "$build"
if [ ! -f Makefile ]; then
    ../configure --with-pydebug --prefix="$HOME/.local"
fi
for target in all test; do
    make -j"$(nproc)" "$target" || exit 1
done
echo "Built in $build"
