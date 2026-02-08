#!/bin/sh
# This script is a convience for local development as it does it all: 
# - Ensures the latest rust code is built
# - Builds the full static site
# - Opens the browser
#
# It's smart enough to check if the port is already serving before
# bringing it up again (which would fail)
#
# Usage: ./local-build-and-serve.sh <up|down>

set -e

if [ -z "$1" ] || { [ "$1" != "up" ] && [ "$1" != "down" ]; }; then
    echo "Usage: $0 <up|down>"
    exit 1
fi

if [ "$1" = "up" ]; then
    # Build CLI and static site
    cargo build && target/debug/simple-gal build

    # Start server only if not already running
    if ! lsof -i :8000 -sTCP:LISTEN >/dev/null 2>&1; then
        python3 -m http.server 8000 -d dist > /dev/null 2>&1 &
        sleep 1  # Give server time to start
    fi

    # Open browser
    open http://localhost:8000

elif [ "$1" = "down" ]; then
    # Kill python http server on port 8000 if running
    PID=$(lsof -ti :8000 -sTCP:LISTEN 2>/dev/null || true)
    if [ -n "$PID" ]; then
        kill "$PID"
        echo "Server stopped (PID $PID)"
    else
        echo "No server running on port 8000"
    fi
fi
