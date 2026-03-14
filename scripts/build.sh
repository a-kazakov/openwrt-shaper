#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION=$(git describe --tags --always 2>/dev/null || echo "dev")
LDFLAGS="-s -w -X main.version=${VERSION}"

echo "Building slqm ${VERSION}..."

mkdir -p dist

targets=(
    "linux/arm64/"
    "linux/arm/7"
    "linux/mipsle/"
)

for target in "${targets[@]}"; do
    IFS='/' read -r goos goarch goarm <<< "$target"
    suffix="${goarch}"
    [ -n "$goarm" ] && suffix="${goarch}v${goarm}"

    echo "  → ${goos}/${goarch}${goarm:+v$goarm}"

    GOOS=$goos GOARCH=$goarch ${goarm:+GOARM=$goarm} \
        GOMIPS=${goarch:+$([ "$goarch" = "mipsle" ] && echo "softfloat")} \
        CGO_ENABLED=0 \
        go build -ldflags="$LDFLAGS" -o "dist/slqm-${suffix}" ./cmd/slqm
done

echo "Build complete. Binaries in dist/:"
ls -lh dist/
