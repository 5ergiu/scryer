#!/usr/bin/env bash
# Wrap an already-built (and, for releases, already-signed) Scryer.app in the
# branded disk image users actually download.
#
# dmgbuild is used rather than Finder/AppleScript layout: it writes the .DS_Store
# directly, so the result does not depend on a logged-in window server and is
# reproducible on a CI runner. Its inputs are hash-pinned in
# dmgbuild-requirements.txt.
#
# The app is copied in as-is. Signing the image is the caller's job, and it has
# to happen after this script runs — the signature covers the finished image.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: build-dmg.sh --app <path/to/Scryer.app> --output <path/to/scryer-darwin-<arch>.dmg>

  --app     Path to the assembled Scryer.app bundle.
  --output  Path of the disk image to write; an existing file is replaced.
  --venv    Optional directory for the pinned dmgbuild virtualenv. Defaults to
            a temporary directory that is removed on exit.
USAGE
  exit 2
}

app=""
output=""
venv=""

while [ $# -gt 0 ]; do
  case "$1" in
    --app) app="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --venv) venv="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

[ -n "$app" ] || usage
[ -n "$output" ] || usage

if [ ! -d "$app" ]; then
  echo "not an app bundle: $app" >&2
  exit 1
fi
if [ ! -x "$app/Contents/MacOS/scryer-tray" ]; then
  echo "bundle is missing Contents/MacOS/scryer-tray: $app" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
assets_dir="$script_dir/assets"

for asset in scryer.icns dmg-background.tiff; do
  if [ ! -f "$assets_dir/$asset" ]; then
    echo "missing brand asset: $assets_dir/$asset" >&2
    exit 1
  fi
done

app="$(cd "$(dirname "$app")" && pwd)/$(basename "$app")"
output_dir="$(cd "$(dirname "$output")" && pwd)"
output="$output_dir/$(basename "$output")"

scratch=""
cleanup() {
  if [ -n "$scratch" ]; then
    rm -rf "$scratch"
  fi
}
trap cleanup EXIT

if [ -z "$venv" ]; then
  scratch="$(mktemp -d)"
  venv="$scratch/dmgvenv"
fi

if [ ! -x "$venv/bin/dmgbuild" ]; then
  python3 -m venv "$venv"
  "$venv/bin/python" -m pip install --quiet --upgrade pip
  "$venv/bin/python" -m pip install --quiet --require-hashes \
    -r "$script_dir/dmgbuild-requirements.txt"
fi

rm -f "$output"
"$venv/bin/dmgbuild" \
  --settings "$script_dir/dmg-settings.py" \
  -D "app=$app" \
  -D "assets=$assets_dir" \
  Scryer \
  "$output"

test -f "$output"
echo "built $output"
