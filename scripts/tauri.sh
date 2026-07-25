#!/bin/sh
# Wrapper for every Tauri CLI call (`npm run tauri …`). Two jobs:
#
#   1. Load .env, so local build settings reach dev and release builds alike.
#      The Tauri CLI does NOT read .env itself — verified: a build with only
#      .env set still signed ad-hoc.
#   2. Sign macOS bundles with the local "flit-dev" identity unless .env
#      names another one. Ad-hoc signing gives the app a new cdhash on every
#      build, so macOS treats each build as a different application and every
#      account's Keychain "Always Allow" is asked again.
#
# why a default instead of requiring .env: the identity lives in the user's
# keychain, not in the repo, and .env is gitignored — a fresh clone (a new
# Conductor workspace) would otherwise silently go back to ad-hoc builds.
#
# No such identity in the keychain (CI, another Mac) → Tauri's own default.
# A build must never fail over signing.
set -e

if [ -f .env ]; then
  set -a
  . ./.env
  set +a
fi

: "${APPLE_SIGNING_IDENTITY:=flit-dev}"
if security find-identity -p codesigning -v 2>/dev/null |
  grep -q "\"$APPLE_SIGNING_IDENTITY\""; then
  export APPLE_SIGNING_IDENTITY
else
  echo "tauri: no \"$APPLE_SIGNING_IDENTITY\" identity — build stays ad-hoc signed" >&2
  unset APPLE_SIGNING_IDENTITY
fi

exec ./node_modules/.bin/tauri "$@"
