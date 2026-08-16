#!/bin/sh
# Run the built release app with timing switched on, keeping the output.
#
# why not just double-clicking the bundle: a .app launched from Finder
# inherits no shell environment, so FLIT_TIMING would never reach it. This
# starts the bundle's binary directly, which also puts its stderr in the
# terminal — where it can be read and kept.
#
# why the release bundle and not `npm run tauri dev`: a debug binary is a
# different, far slower program. Measuring it would send us optimising paths
# that are not slow in the build people actually run. That is also why the
# switch is an env var rather than #[cfg(debug_assertions)].
#
# Both halves of a click are covered: the backend reads FLIT_TIMING, and the
# webview mirrors it at startup (see src/main.ts), so the console shows Rust
# and frontend lines interleaved in one stream.
set -e

# why lowercase "flit": the bundle is named from productName ("Flit"), the
# binary inside it from the crate ("flit"). A case-insensitive volume hides
# the difference locally — it would not stay hidden everywhere.
app="src-tauri/target/release/bundle/macos/Flit.app/Contents/MacOS/flit"
if [ ! -x "$app" ]; then
  echo "no release build yet — run: npm run tauri build" >&2
  exit 1
fi

# why outside the repo by default: this is a throwaway artefact of a
# measuring run, and it must not end up in a commit.
# why the %/ trim: macOS sets TMPDIR with a trailing slash, /tmp has none.
tmp="${TMPDIR:-/tmp}"
log="${1:-${tmp%/}/flit-timing.log}"
echo "timing → $log" >&2

FLIT_TIMING=1 "$app" 2>&1 | tee "$log"

echo "timing log: $log" >&2
