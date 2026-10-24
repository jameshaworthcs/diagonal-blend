#!/bin/sh
# Run a command with an X display, which the headed Firefox project needs.
# Uses $DISPLAY if set, otherwise xvfb-run; without either, the Firefox
# project is left out and the rest still runs.
set -eu
if [ -n "${DISPLAY:-}" ]; then
  exec "$@"
fi
if command -v xvfb-run >/dev/null 2>&1; then
  exec xvfb-run -a -s "-screen 0 1600x1000x24" "$@"
fi
echo "with-display: no display and no xvfb-run, so Firefox is skipped" >&2
exec "$@"
