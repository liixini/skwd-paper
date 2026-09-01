#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
VERIFY_ROOT="${SKWD_VERIFY_ROOT:-../skwd-verify}"
if [ ! -f "$VERIFY_ROOT/scripts/python_suite.py" ]; then
    echo "missing skwd-verify checkout at $VERIFY_ROOT (set SKWD_VERIFY_ROOT)" >&2
    exit 1
fi
exec python3 "$VERIFY_ROOT/scripts/python_suite.py" python-general
