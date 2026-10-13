#!/bin/sh
# Build the generic Linux download: make-tarball.sh VERSION ARCH PATH_TO_BINARY [OUT_DIR]
set -eu
VERSION=$1; ARCH=$2; BINARY=$3; OUT=${4:-dist}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
NAME="phone-remote-$VERSION-linux-$ARCH"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/$NAME" "$OUT"
install -m 755 "$BINARY" "$STAGE/$NAME/phone-remote"
install -m 755 "$HERE/install.sh" "$STAGE/$NAME/install.sh"
install -m 644 "$HERE/phone-remote.desktop" "$STAGE/$NAME/phone-remote.desktop"
install -m 644 "$ROOT/installer/assets/phone-remote.png" "$STAGE/$NAME/phone-remote.png"
install -m 644 "$ROOT/LICENSE" "$STAGE/$NAME/LICENSE"
install -m 644 "$ROOT/README.md" "$STAGE/$NAME/README.md"
tar -C "$STAGE" -czf "$OUT/$NAME.tar.gz" "$NAME"
echo "$OUT/$NAME.tar.gz"
