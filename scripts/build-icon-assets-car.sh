#!/usr/bin/env bash
# Compiles src-tauri/icons/AppIcon.icon (the Icon Composer / Liquid Glass
# source) into src-tauri/icons/AppIcon.car, which tauri.conf.json's
# bundle.icon references directly.
#
# why a pre-built .car instead of listing AppIcon.icon in tauri.conf.json:
# actool/ibtoold (Apple's compiler, invoked internally by the Tauri bundler)
# crashes deterministically with "attempt to insert nil object from
# objects[0]" when invoked from inside the cargo/tauri-cli process tree on
# this machine, but succeeds every time run directly from an interactive
# shell like this script. Pre-building the .car here and pointing
# tauri.conf.json at the .car file sidesteps the crash entirely — Tauri's
# bundler just copies an already-compiled .car, no actool invocation needed.
#
# Run this after editing src-tauri/icons/AppIcon.icon, then rebuild the app.
set -e
cd "$(dirname "$0")/.."

# A previous actool run can leave ibtoold (its background XPC compiler
# daemon) alive; killing it first avoids the crash above.
pkill -9 -f ibtoold >/dev/null 2>&1 || true
sleep 1

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

xcrun actool src-tauri/icons/AppIcon.icon --compile "$OUT" \
  --output-format human-readable-text --notices --warnings \
  --output-partial-info-plist "$OUT/assetcatalog_generated_info.plist" \
  --app-icon Icon --include-all-app-icons --accent-color AccentColor \
  --enable-on-demand-resources NO --development-region en \
  --target-device mac --minimum-deployment-target 26.0 --platform macosx

cp "$OUT/Assets.car" src-tauri/icons/AppIcon.car
echo "Wrote src-tauri/icons/AppIcon.car"
