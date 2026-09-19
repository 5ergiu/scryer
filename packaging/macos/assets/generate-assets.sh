#!/usr/bin/env bash
# Regenerate the committed macOS brand assets from the Scryer brand art pack.
#
# This is NOT run in CI. scryer.icns, dmg-background.tiff and the menu-bar
# glyphs are committed because they are reviewed artwork, and because a release
# must not depend on a rasterizer version. The source pack itself is ~18 MB of
# print-resolution art and is NOT committed: pass its extracted directory (or
# the zip) with --brand when the artwork changes.
#
# Requires `brew install imagemagick`; sips, iconutil and tiffutil ship with
# macOS. `unzip` is only needed when --brand names a zip.
#
# --- the naming trap --------------------------------------------------------
#
# The tray's resource files are named, like Weaver's, for the MENU-BAR
# APPEARANCE THEY SERVE, not for their own colour. A white glyph is what a dark
# menu bar needs, so the mapping is CROSSED:
#
#   white glyph  ->  menubar-dark{,@2x}.png
#   black glyph  ->  menubar-light{,@2x}.png
#
# Both are drawn from the single-colour macOS mark (--menubar), not from the
# brand pack: the pack's colour marks turn to mush at 18 points and look out of
# place beside the system's monochrome menu-bar icons.
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: generate-assets.sh --brand <dir-or-zip> --menubar <svg> [--output <dir>]

  --brand   The extracted Scryer brand art pack, or the zip itself. Must
            contain "Light Theme" and "Dark Theme" directories.
  --menubar The single-colour macOS menu-bar mark as SVG (MacOS-B-W-Logo.svg).
  --output  Directory to write scryer.icns and dmg-background.tiff into.
            Defaults to this script's directory, i.e. it overwrites the
            committed assets in place. The menu-bar glyphs always go to
            crates/scryer/resources/macos, where the tray `include_bytes!`s
            them.
USAGE
  exit 2
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
output_dir="$script_dir"
brand=""
menubar_mark=""

while [ $# -gt 0 ]; do
  case "$1" in
    --brand) brand="${2:-}"; shift 2 ;;
    --menubar) menubar_mark="${2:-}"; shift 2 ;;
    --output) output_dir="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done

[ -n "$brand" ] || usage
[ -f "$menubar_mark" ] || usage
[ -n "$output_dir" ] || usage
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

for tool in magick sips iconutil tiffutil; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "$tool not found; brew install imagemagick" >&2
    exit 1
  }
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ -f "$brand" ]; then
  command -v unzip >/dev/null 2>&1 || { echo "unzip not found" >&2; exit 1; }
  unzip -q -o "$brand" -d "$work/brand"
  brand="$work/brand"
fi
[ -d "$brand" ] || { echo "not a directory: $brand" >&2; exit 1; }

# The 2x rasters are the largest bitmaps in the pack (2401px square), so every
# downscale below starts from them rather than from a smaller frame.
white_mark="$brand/Light Theme/2x/Logo@2xORG1200X.png"
navy_mark="$brand/Dark Theme/2x/Logo_1@2xORG1200X.png"
wordmark="$brand/Dark Theme/2x/Text_1@2xORG1200X.png"
for source in "$white_mark" "$navy_mark" "$wordmark"; do
  [ -f "$source" ] || { echo "missing brand source: $source" >&2; exit 1; }
done

# The pack centres each mark inside a 1201pt canvas with transparent slack
# around it. Every destination here wants the ink at a given size, so the slack
# is trimmed and the ink re-padded to a square before it is scaled down —
# otherwise an 18px glyph would be mostly empty canvas and read as tiny.
#
# Lanczos is what keeps the ring's thin gaps open at 18px; the default filter
# closes them into a disc.
square_ink() {
  local source="$1" size="$2" out="$3"
  magick "$source" -trim +repage \
    -background none -gravity center -extent "%[fx:max(w,h)]x%[fx:max(w,h)]" \
    -filter Lanczos -resize "${size}x${size}" \
    -strip -define png:exclude-chunk=date,time "$out"
}

# --- the macOS menu bar ------------------------------------------------------
#
# Named for the appearance each one serves, not for its own colour — see the
# naming trap at the top of this file. The tray picks between them itself, so
# these ship as finished artwork rather than as a template mask for AppKit to
# tint. The SVG is rasterized large first so the downscale, not the
# rasterizer, decides the 18px edges; only its alpha is kept, then filled solid.
magick -background none -density 600 "$menubar_mark" -resize 2048x2048 \
  -alpha extract "$work/menubar-alpha.png"
for tone in white black; do
  magick -size 2048x2048 "xc:$tone" "$work/menubar-alpha.png" \
    -alpha off -compose copy_opacity -composite "$work/menubar-$tone.png"
done
menubar_dir="$repo_root/crates/scryer/resources/macos"
mkdir -p "$menubar_dir"
square_ink "$work/menubar-white.png" 18 "$menubar_dir/menubar-dark.png"
square_ink "$work/menubar-white.png" 36 "$menubar_dir/menubar-dark@2x.png"
square_ink "$work/menubar-black.png" 18 "$menubar_dir/menubar-light.png"
square_ink "$work/menubar-black.png" 36 "$menubar_dir/menubar-light@2x.png"

# --- scryer.icns -------------------------------------------------------------
#
# The navy-outlined mark, which is the variant crates/scryer/resources/windows/
# scryer.ico already ships, so the Dock and the Windows taskbar show the same
# drawing. Apple's icon grid puts the artwork in an 824px box inside the 1024px
# canvas, which is what keeps Scryer's icon the same visual size as the
# system's in the Dock. The mark is a freestanding drawing rather than a tile,
# so it keeps its transparency instead of being masked into a squircle.
square_ink "$navy_mark" 824 "$work/ink-824.png"
magick -size 1024x1024 xc:none "$work/ink-824.png" \
  -gravity center -compose over -composite \
  -strip -define png:exclude-chunk=date,time "$work/master-1024.png"

iconset="$work/scryer.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/master-1024.png" \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$work/master-1024.png" \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil --convert icns "$iconset" --output "$output_dir/scryer.icns"

# --- dmg-background.tiff -----------------------------------------------------
#
# The arrow has to land between the two icon positions in dmg-settings.py
# (Scryer.app at 140,210 and Applications at 460,210 in 1x window points), so
# these coordinates and that file move together. The art is light-themed
# because Finder paints filename labels black over any custom background
# picture — dark art makes the labels unreadable, and that is why the wordmark
# placed here is the pack's dark-theme (navy) one.
draw_background() {
  local scale="$1" out="$2"
  local width=$((600 * scale)) height=$((400 * scale))
  local wordmark_height=$((20 * scale)) offset=$((40 * scale))
  local ax=$((240 * scale)) ay=$((210 * scale))
  local bx=$((330 * scale)) tipx=$((362 * scale))
  local shaft=$((5 * scale)) head=$((16 * scale))

  # The arrow is a single polygon drawn opaque on its own layer and faded once
  # on composite — a stroked shaft plus a filled head would double-composite
  # their translucent alphas where they overlap and leave a seam.
  magick -size "${width}x${height}" xc:none -fill '#2d3850' \
    -draw "polygon $ax,$((ay - shaft)) $bx,$((ay - shaft)) $bx,$((ay - head)) $tipx,$ay $bx,$((ay + head)) $bx,$((ay + shaft)) $ax,$((ay + shaft))" \
    -channel A -evaluate multiply 0.30 +channel "$work/arrow${scale}x.png"

  # Trimmed to its ink before placement, or the offset below would be measured
  # from empty canvas. Its alpha is faded once, before compositing, for the
  # same reason the arrow's is.
  magick "$wordmark" -trim +repage -filter Lanczos -resize "x${wordmark_height}" \
    -channel A -evaluate multiply 0.55 +channel "$work/wordmark${scale}x.png"

  magick -size "${width}x${height}" gradient:'#ffffff-#eef1f6' \
    "$work/arrow${scale}x.png" -compose over -composite \
    "$work/wordmark${scale}x.png" -gravity north -geometry "+0+$offset" -composite \
    -strip "$out"
}

draw_background 1 "$work/bg1x.png"
draw_background 2 "$work/bg2x.png"

# A single TIFF carrying both representations is how Finder is told which one
# is the Retina image; two separate files would leave the 1x art upscaled.
tiffutil -cathidpicheck "$work/bg1x.png" "$work/bg2x.png" \
  -out "$output_dir/dmg-background.tiff"

echo "wrote $menubar_dir/menubar-{light,dark}{,@2x}.png"
echo "wrote $output_dir/scryer.icns"
echo "wrote $output_dir/dmg-background.tiff"
