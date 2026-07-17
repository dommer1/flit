#!/bin/sh
# Cargo runner for dev builds on macOS. Signs the freshly built binary with the
# local self-signed "flit-dev" identity before launching it, so the Keychain's
# "Always Allow" ACL survives rebuilds (an ad-hoc-signed binary gets a new
# cdhash every build and re-triggers the password prompt for every account).
# Skips signing when the identity doesn't exist (other machines, CI).
set -e
if security find-identity -p codesigning -v 2>/dev/null | grep -q '"flit-dev"'; then
  codesign -f -s flit-dev "$1"
fi
exec "$@"
