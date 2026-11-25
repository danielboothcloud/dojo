#!/bin/sh
# Push a commit straight to the forge "from outside" the session
# (for example a reviewer who pushes to the PR branch on GitHub).
#
# Usage: outside_push.sh <layout-dir> <branch> <file> <line>
set -eu
cd "$1/outside"
g() { git -c user.name=Outside -c user.email=outside@example.com "$@"; }
g fetch -q origin
g checkout -q -B "$2" "origin/$2"
printf '%s\n' "$4" >> "$3"
g add -- "$3"
g commit -q -m "outside: edit $3"
g push -q origin "$2"
