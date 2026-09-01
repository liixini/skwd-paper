#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
component=${1:-all}

case "$component" in
    vk)
        packages="skwd-wall-vk"
        ;;
    still)
        packages="skwd-wall-still"
        ;;
    tinier)
        packages="paper-tinier"
        ;;
    all)
        packages="skwd-wall-vk skwd-wall-still paper-tinier"
        ;;
    *)
        echo "usage: $0 {vk|still|tinier|all}" >&2
        exit 2
        ;;
esac

set --
for package in $packages; do
    set -- "$@" -p "$package"
done

exec cargo build --manifest-path "$root/Cargo.toml" --release --locked "$@"
