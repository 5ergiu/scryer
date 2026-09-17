#!/usr/bin/env bash
# Assemble Scryer.app around already-built binaries.
#
# The bundle is a wrapper, not a second build: the executables it contains are
# the same `scryer` and `scryer-tray` the portable tarball ships.
#
# The finished bundle is ad-hoc signed here because that is Scryer's shipping
# path: there is no Developer ID certificate, and arm64 macOS refuses to run
# Mach-O code carrying no signature at all, while WKWebView needs one coherent
# signature over the whole bundle rather than the per-binary ones Rust's linker
# leaves behind. A release that *does* have a certificate re-signs over this,
# which is exactly what codesign --force is for.
#
# Only tools that ship with macOS are used, so this runs on any Mac.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: build-app.sh --version X.Y.Z --scryer <path> --tray <path> --output <dir>

  --version  Version string written into CFBundleShortVersionString/CFBundleVersion.
  --scryer   Path to the built `scryer` server binary.
  --tray     Path to the built `scryer-tray` desktop wrapper binary.
  --output   Directory the bundle is created in; Scryer.app is placed inside it.
USAGE
  exit 2
}

version=""
scryer_binary=""
tray_binary=""
output_dir=""

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --scryer) scryer_binary="${2:-}"; shift 2 ;;
    --tray) tray_binary="${2:-}"; shift 2 ;;
    --output) output_dir="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

[ -n "$version" ] || usage
[ -n "$scryer_binary" ] || usage
[ -n "$tray_binary" ] || usage
[ -n "$output_dir" ] || usage

# The version lands in Info.plist through an unquoted sed substitution, and
# Launch Services rejects bundles whose version strings are not plain dotted
# numbers — so reject anything else before it is baked into a bundle.
case "$version" in
  *[!0-9.]*|.*|*.|*..*|"")
    echo "version must be release-derived major.minor.patch, got: $version" >&2
    exit 1
    ;;
esac

for binary in "$scryer_binary" "$tray_binary"; do
  if [ ! -f "$binary" ]; then
    echo "not a file: $binary" >&2
    exit 1
  fi
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
icon_source="$script_dir/assets/scryer.icns"

if [ ! -f "$icon_source" ]; then
  echo "missing app icon at $icon_source" >&2
  exit 1
fi

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
bundle="$output_dir/Scryer.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"

# Launch Services reads CFBundleExecutable, so the wrapper has to keep its own
# name inside the bundle; the server sits beside it because that is where the
# wrapper looks for it.
install -m 0755 "$tray_binary" "$bundle/Contents/MacOS/scryer-tray"
install -m 0755 "$scryer_binary" "$bundle/Contents/MacOS/scryer"

# The icon is committed rather than rendered here: reviewed artwork should not
# be re-rasterized by every build. assets/generate-assets.sh reproduces it.
install -m 0644 "$icon_source" "$bundle/Contents/Resources/scryer.icns"

sed -e "s/@VERSION@/$version/g" "$script_dir/Info.plist" > "$bundle/Contents/Info.plist"
plutil -lint "$bundle/Contents/Info.plist"

# `install` copies the file bytes, so each binary keeps the ad-hoc signature
# Rust's linker gave it — but those signatures do not cover Info.plist or the
# Resources, so the bundle as a whole is unsigned until this runs. No hardened
# runtime and no timestamp: both are meaningful only with a real identity, and
# the hardened runtime on an ad-hoc signature just adds restrictions no
# notarization ever relaxes.
codesign --sign - --force --deep --timestamp=none "$bundle"
codesign --verify --deep --strict "$bundle"

echo "built $bundle"
