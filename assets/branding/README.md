# Branding / app icon

The logo is a butler's shirtfront: a dark jacket tile, a white shirt, and an
emerald bow tie. The SVGs here are the source of truth; every PNG, ICNS, and
ICO is generated from them.

| Path | Role |
|------|------|
| `icon.svg` | Master on Apple's icon grid: 100px margin on the 1024 canvas, soft shadow. Input for the macOS ICNS |
| `icon-flat.svg` | Same art, tightly cropped with no soft shadow, because Windows and Linux add no shape or margin. Input for `bunx tauri icon` |
| `tray-bow-tie.svg` | One-colour menu-bar silhouette |
| `../../src/assets/brand/alfred-bow-tie.svg` | Bow tie alone, for the Speak with Alfred orb |
| `../../src-tauri/icons/*` | Generated platform icons |
| `../../public/icon.png` | Favicon, copied from `128x128@2x.png` |

## Regenerate

Run from the repository root. Run the flat step first, because it also writes
an `icon.icns` that the macOS step replaces.

```bash
bunx tauri icon assets/branding/icon-flat.svg
cp src-tauri/icons/128x128@2x.png public/icon.png

work=$(mktemp -d)
bunx tauri icon assets/branding/icon.svg -o "$work/out" --png 1024
mkdir "$work/Alfred.iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/out/1024x1024.png" --out "$work/Alfred.iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$work/out/1024x1024.png" --out "$work/Alfred.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
swift scripts/package-macos-icns.swift "$work/Alfred.iconset" src-tauri/icons/icon.icns

bunx tauri icon assets/branding/tray-bow-tie.svg -o "$work/tray" --png 64
cp "$work/tray/64x64.png" src-tauri/icons/trayBowTieTemplate@2x.png
rm -rf "$work"
```

The menu-bar PNG is a macOS template image, so only its alpha channel matters.
