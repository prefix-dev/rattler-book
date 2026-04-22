#!/usr/bin/env bash
set -euo pipefail

# Records casts/build.cast showing:
#   $ shot build --output-dir ../build-channel
#
# Runs in /tmp/shot-demo/lumen-build/ (freshly copied from examples/lumen/)
# so the build output is "cold" and doesn't collide with prep's channel.

REPO="$PWD"
CAST="$REPO/casts/build.cast"
SHOT="$REPO/target/release/shot"
BUILD_DIR=/tmp/shot-demo/lumen-build
CHANNEL_DIR=/tmp/shot-demo/build-channel

mkdir -p "$REPO/casts"
rm -rf "$BUILD_DIR" "$CHANNEL_DIR"
cp -r "$REPO/examples/lumen" "$BUILD_DIR"
mkdir -p "$CHANNEL_DIR"

INNER="$(mktemp -t shot-cast-build-XXXXXX.sh)"
trap 'rm -f "$INNER"' EXIT

cat > "$INNER" <<INNER_EOF
#!/usr/bin/env bash
set -e
export PS1='\$ '
export RUST_LOG=error
SHOT='$SHOT'

echo "\$ shot build --output-dir ../build-channel"
"\$SHOT" build --output-dir ../build-channel
INNER_EOF
chmod +x "$INNER"

cd "$BUILD_DIR"
asciinema rec "$CAST" \
    --cols 100 --rows 30 --overwrite --idle-time-limit 2 \
    --command "bash --noprofile --norc $INNER"

echo "record-build: wrote $CAST"
