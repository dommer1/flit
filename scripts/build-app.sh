#!/bin/sh
# Release bundler for macOS. Same problem as scripts/dev-runner.sh, other end
# of the pipeline: Tauri signs the bundle ad-hoc by default, which gives the
# app a new cdhash on every build, so macOS sees a different application each
# time and every account's Keychain "Always Allow" is asked again. Signing
# with the stable local "flit-dev" identity keeps one ACL across rebuilds.
#
# Falls through to Tauri's own default when the identity is missing (other
# machines, CI) — an unsigned build is better than a failed one.
set -e
if security find-identity -p codesigning -v 2>/dev/null | grep -q '"flit-dev"'; then
  APPLE_SIGNING_IDENTITY=flit-dev
  export APPLE_SIGNING_IDENTITY
else
  echo "build-app: no flit-dev identity, leaving the bundle ad-hoc signed" >&2
fi
exec npm run tauri build -- "$@"
