#!/bin/sh
# Build a local copy of the Sundae session layout in "$1":
#
#   $1/forge.git    bare repo: the forge (GitHub/GitLab stand-in)
#   $1/sundae.git   bare repo: the Sundae server. Its hooks copy the Sundae
#                   push rules and forward accepted pushes to the forge.
#   $1/work         colocated jj repo (jj git clone --colocate) with
#                     origin = forge (fetch only; push URL is a dead file URL)
#                     sundae = sundae.git (fetch + push)
#                   and NO per-repo jj config (git.push / git.fetch unset).
#   $1/outside      plain git clone of the forge, to push "from outside".
#
# Forge content: `main` (2 commits) and `fix/goals` (1 commit on main).
# Sundae rules (refusal messages match the real server):
#   - protected bookmarks (sundae.git/sundae-protected, default: main)
#   - "forge <b> has commits you lack" when the forge tip is not an
#     ancestor of the pushed commit
#   - commits that add private paths (sundae.git/sundae-private)
#
# Usage: sundae_layout.sh <dir>     (needs git and jj on PATH)
set -eu

root=$1
mkdir -p "$root"
cd "$root"
root=$(pwd)

git_q() { git -c init.defaultBranch=main -c user.name=Forge -c user.email=forge@example.com "$@"; }

# --- forge -----------------------------------------------------------------
git_q init -q --bare forge.git
git_q init -q seed
(
    cd seed
    echo "hello" > README.md
    git_q add README.md
    git_q commit -q -m "initial"
    echo "goals: none" > goals.txt
    git_q add goals.txt
    git_q commit -q -m "add goals"
    git_q checkout -q -b fix/goals
    echo "goals: ship dojo" > goals.txt
    git_q commit -q -am "fix goals"
    git_q checkout -q main
    git_q push -q "$root/forge.git" main fix/goals
)
rm -rf seed

# --- sundae ----------------------------------------------------------------
git_q clone -q --bare forge.git sundae.git
printf '%s\n' "$root/forge.git" > sundae.git/sundae-forge
printf '%s\n' main > sundae.git/sundae-protected
printf '%s\n' AGENTS.md todo.txt .pi/ _bmad/ _bmad-output/ > sundae.git/sundae-private

cat > sundae.git/hooks/pre-receive <<'HOOK'
#!/bin/sh
# Sundae push rules (test copy). Messages match the real server.
zero=0000000000000000000000000000000000000000
forge=$(cat "$GIT_DIR/sundae-forge")
fail=0
out=$(mktemp)
while read -r old new ref; do
    case "$ref" in refs/heads/*) ;; *) continue ;; esac
    b=${ref#refs/heads/}
    if grep -qx -- "$b" "$GIT_DIR/sundae-protected"; then
        echo "sundae: refusing to update $b: $b is protected" >&2
        fail=1
        continue
    fi
    [ "$new" = "$zero" ] && continue
    fsha=$(
        unset GIT_DIR GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_QUARANTINE_PATH
        git --git-dir="$forge" rev-parse -q --verify "refs/heads/$b^{commit}" 2>/dev/null || true
    )
    if [ -n "$fsha" ]; then
        if ! git cat-file -e "$fsha^{commit}" 2>/dev/null ||
            ! git merge-base --is-ancestor "$fsha" "$new"; then
            echo "sundae: forge $b has commits you lack; run jj git fetch, rebase onto $b@origin, and push again" >&2
            fail=1
            continue
        fi
    fi
    : > "$out"
    for c in $(git rev-list "$new" --not --all); do
        git diff-tree --no-commit-id --name-only -r --diff-filter=A --root "$c" |
            while IFS= read -r p; do
                while IFS= read -r priv; do
                    [ -n "$priv" ] || continue
                    case "$priv" in
                        */) case "$p" in "$priv"*) echo "$c $p" ;; esac ;;
                        *) [ "$p" = "$priv" ] && echo "$c $p" ;;
                    esac
                done < "$GIT_DIR/sundae-private"
            done >> "$out"
    done
    if [ -s "$out" ]; then
        while read -r c p; do
            echo "sundae: commit $c adds private path $p; private files live in refs/sundae/private and must not be committed" >&2
        done < "$out"
        fail=1
    fi
done
rm -f "$out"
exit $fail
HOOK

cat > sundae.git/hooks/post-receive <<'HOOK'
#!/bin/sh
# Forward accepted bookmark updates to the forge in the same push.
zero=0000000000000000000000000000000000000000
forge=$(cat "$GIT_DIR/sundae-forge")
while read -r old new ref; do
    case "$ref" in refs/heads/*) ;; *) continue ;; esac
    if [ "$new" = "$zero" ]; then
        git push -q "$forge" ":$ref" >&2 || echo "sundae: forward of $ref failed" >&2
    else
        git push -q "$forge" "$new:$ref" >&2 || echo "sundae: forward of $ref failed" >&2
    fi
    echo "sundae: forwarded ${ref#refs/heads/} to the forge" >&2
done
HOOK
chmod +x sundae.git/hooks/pre-receive sundae.git/hooks/post-receive

# --- session clone -----------------------------------------------------------
jj git clone --quiet --colocate "$root/forge.git" work 2>/dev/null
(
    cd work
    git remote set-url --push origin file:///sundae-forwards-pushes/use-jj-git-push-to-sundae
    jj git remote add sundae "$root/sundae.git"
    jj git fetch --remote sundae --quiet 2>/dev/null || jj git fetch --remote sundae
    # Like a Sundae shell: NO per-repo jj config (no git.push / git.fetch,
    # not even the trunk() alias that `jj git clone` writes).
    repo_config=$(jj config path --repo 2>/dev/null || true)
    [ -n "$repo_config" ] && rm -f "$repo_config"
    # Private files: excluded and never seen by jj status.
    printf '%s\n' AGENTS.md .pi/ >> .git/info/exclude
    echo "agent notes" > AGENTS.md
)

# --- outside clone (pushes straight to the forge) ----------------------------
git_q clone -q forge.git outside
