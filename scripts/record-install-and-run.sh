#!/usr/bin/env bash
set -euo pipefail

# Records casts/install-and-run.cast showing:
#   $ shot init my-app --channel ../channel --channel conda-forge
#   $ shot add lumen
#   $ shot install
#   $ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"

REPO="$PWD"
CAST="$REPO/casts/install-and-run.cast"
SHOT="$REPO/target/release/shot"
WORK=/tmp/shot-demo/work

mkdir -p "$REPO/casts"

# shot init writes moonshot.toml into the CWD (naming the project `my-app`).
# It does not create a subdirectory. Wipe any state from a previous recording
# so init doesn't bail with "already exists".
rm -f "$WORK/moonshot.toml" "$WORK/moonshot.lock"
rm -rf "$WORK/.env"

# Inner script that asciinema records. Written to a temp file so we don't
# have to juggle nested shell quoting.
INNER="$(mktemp -t shot-cast-install-XXXXXX.sh)"
trap 'rm -f "$INNER"' EXIT

cat > "$INNER" <<INNER_EOF
#!/usr/bin/env bash
set -e
export PS1='\$ '
export RUST_LOG=error
SHOT='$SHOT'

echo "\$ shot init my-app --channel ../channel --channel conda-forge"
"\$SHOT" init my-app --channel ../channel --channel conda-forge

echo
echo "\$ shot add lumen"
"\$SHOT" add lumen

echo
echo "\$ shot install"
"\$SHOT" install

echo
echo "\$ shot run lua -e \"require('lumen').thumbnail('photo.jpg', 128)\""
"\$SHOT" run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
INNER_EOF
chmod +x "$INNER"

cd "$WORK"
asciinema rec "$CAST" \
    --cols 100 --rows 30 --overwrite --idle-time-limit 2 \
    --command "bash --noprofile --norc $INNER"

echo "record-install-and-run: wrote $CAST"
