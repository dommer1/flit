#!/bin/sh
# Cargo runner for dev builds on macOS. Signs the freshly built binary with the
# local self-signed identity before launching it, so the Keychain's "Always
# Allow" ACL survives rebuilds (an ad-hoc-signed binary gets a new cdhash every
# build and re-triggers the password prompt for every account).
#
# The identity is the one scripts/tauri.sh resolved (from .env, else the
# "flit-dev" default) and exported, so dev binaries and release bundles carry
# the same signature. Skips signing when it doesn't exist (other machines, CI).
set -e
identity="${APPLE_SIGNING_IDENTITY:-flit-dev}"
if security find-identity -p codesigning -v 2>/dev/null | grep -q "\"$identity\""; then
  codesign -f -s "$identity" "$1"
fi
exec "$@"
