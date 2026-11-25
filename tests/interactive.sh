#!/usr/bin/env bash
# Scaffolds a jj repo with workspaces created through dojo for manual TUI testing.
# Usage: task playground
set -euo pipefail
export DOJO_LOG=1

DIR="$(mktemp -d)"
DIRECTIVE="$(mktemp)"
trap 'rm -f "$DIRECTIVE"' EXIT

REPO="$DIR/repo/eg"
mkdir -p "$REPO"

# --- init repo ---
jj git init "$REPO"
jj -R "$REPO" config set --repo user.name "Test"
jj -R "$REPO" config set --repo user.email "test@test.com"

# --- build some history on default ---
cd "$REPO"
echo "base" > base.txt
jj describe -m "initial commit"
jj new -m "second commit"
echo "second" > second.txt
jj describe -m "second commit"
jj bookmark create main -r @
jj new -m "third: add config"
echo "config = true" > config.txt
jj describe -m "third: add config"
jj new -m "fourth: logging"
echo "log setup" > logging.txt
jj describe -m "fourth: logging"
jj new -m "fifth: error handling"
echo "handle errors" > errors.txt
jj describe -m "fifth: error handling"
jj new -m "wip"

# --- init dojo config ---
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo init
cat > .config/dojo.toml << 'TOML'
workspace-path = "../../workspaces/{{ repo }}.{{ bookmark }}"
TOML

# --- create workspaces through dojo ---
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new feature/alpha --revision main -m "alpha start"
cd "$DIR/workspaces/eg.feature-alpha"
jj new -m "alpha: add widget"
echo "widget" > widget.txt
jj describe -m "alpha: add widget"
jj new -m "alpha: refine widget"
echo "widget v2" > widget.txt
jj describe -m "alpha: refine widget"
jj new -m "wip"

cd "$REPO"
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new feature/beta --revision main -m "beta start"
cd "$DIR/workspaces/eg.feature-beta"
jj new -m "beta: scaffolding"
echo "scaffold" > scaffold.txt
jj describe -m "beta: scaffolding"
jj new -m "beta: implement core"
echo "core" > core.txt
jj describe -m "beta: implement core"
jj new -m "beta: tests"
echo "tests" > tests.txt
jj describe -m "beta: tests"
jj new -m "wip"

cd "$REPO"
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new hotfix/sec-123 --revision main -m "hotfix start"
cd "$DIR/workspaces/eg.hotfix-sec-123"
jj new -m "hotfix: patch security vuln"
echo "patch" > patch.txt
jj describe -m "hotfix: patch security vuln"

cd "$REPO"
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new scratch --revision main -m "scratch workspace"

cd "$REPO"
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new feature/deep --revision @ -m "deep start"
cd "$DIR/workspaces/eg.feature-deep"
jj new -m "deep: schema design"
echo "schema v1" > schema.txt
jj describe -m "deep: schema design"
jj new -m "deep: migrations"
echo "migrate" > migrate.txt
jj describe -m "deep: migrations"
jj new -m "deep: seed data"
echo "seeds" > seeds.txt
jj describe -m "deep: seed data"
jj new -m "wip"

cd "$REPO"
DOJO_DIRECTIVE_FILE="$DIRECTIVE" command dojo new experiment/stale --revision @--- -m "stale experiment"

# --- done ---
echo ""
echo "=== Playground ready ==="
echo "  repo:       $REPO"
echo "  workspaces: $DIR/workspaces/"
echo ""
echo "  cd $REPO && dojo"
