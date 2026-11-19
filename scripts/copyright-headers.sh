#!/usr/bin/env bash
# Copyright 2026 Cru
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Keeps copyright headers in line with NOTICE:
#   - upstream file Cru changed: keep the Google line, add "Modifications Copyright <year> Cru"
#   - new .rs/.sh/workflow file with no header: add a "Copyright <year> Cru" Apache header
#   - new file carrying a Google header: report it (new Cru code, or moved upstream code?)
#
# Usage:
#   scripts/copyright-headers.sh [--check] [FILE...]   (no FILE: every file changed since the fork)
#   scripts/copyright-headers.sh --claude-hook         (Claude Code PostToolUse hook; JSON on stdin)

set -euo pipefail

# Last upstream (googleworkspace/cli) commit the fork is based on.
FORK_POINT=a3768d0e82ad83cca2da97724e46bea4ff0e6dbd
GOOGLE_RE='Copyright [0-9]{4} Google LLC'
CRU_RE='Copyright [0-9]{4} Cru([^A-Za-z]|$)'
YEAR=$(date +%Y)

top=$(git rev-parse --show-toplevel)
top_real=$(cd "$top" && pwd -P)
cd "$top"
if ! git cat-file -e "$FORK_POINT^{commit}" 2>/dev/null; then
  echo "copyright-headers: fork point commit not found (shallow clone? use fetch-depth: 0)" >&2
  exit 1
fi

mode=fix
hook=false
case "${1:-}" in
  --check) mode=check; shift ;;
  --claude-hook) hook=true; shift ;;
esac

# Repo-relative path, or empty if the file is outside this repo.
repo_path() {
  local f=$1 dir
  [ -f "$f" ] || return 0
  dir=$(cd "$(dirname "$f")" && pwd -P)
  case "$dir/" in
    "$top_real"/*) ;;
    *) return 0 ;;
  esac
  local rel="${dir#"$top_real"}/$(basename "$f")"
  rel=${rel#/}
  git check-ignore -q "$rel" && return 0
  printf '%s' "$rel"
}

header_has() { head -n 5 "$1" | grep -Eq "$2"; }
in_fork_point() { git cat-file -e "$FORK_POINT:$1" 2>/dev/null; }

comment_prefix() {
  case "$1" in
    *.rs) echo "//" ;;
    *.sh | .github/workflows/*.yml | .github/workflows/*.yaml) echo "#" ;;
    *) echo "" ;;
  esac
}

add_modifications_line() {
  local f=$1 tmp
  tmp=$(mktemp)
  awk -v re="$GOOGLE_RE" -v year="$YEAR" '
    !done && $0 ~ re {
      print
      prefix = substr($0, 1, index($0, "Copyright") - 1)
      print prefix "Modifications Copyright " year " Cru"
      done = 1
      next
    }
    { print }
  ' "$f" >"$tmp"
  cat "$tmp" >"$f"
  rm -f "$tmp"
}

add_cru_header() {
  local f=$1 p=$2 tmp
  tmp=$(mktemp)
  {
    if head -n 1 "$f" | grep -q '^#!'; then head -n 1 "$f"; fi
    sed "s|^|$p |; s| $||" <<EOF
Copyright $YEAR Cru

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
EOF
    echo
    if head -n 1 "$f" | grep -q '^#!'; then tail -n +2 "$f"; else cat "$f"; fi
  } >"$tmp"
  cat "$tmp" >"$f"
  rm -f "$tmp"
}

problems=0
unfixable=0

process() {
  local f=$1
  header_has "$f" "$CRU_RE" && return 0

  if header_has "$f" "$GOOGLE_RE"; then
    if ! in_fork_point "$f"; then
      echo "copyright-headers: $f is not an upstream file but has a Google LLC header. Use \"Copyright $YEAR Cru\" for new Cru code; keep the Google line and add \"Modifications Copyright $YEAR Cru\" only if the code was moved from upstream." >&2
      unfixable=$((unfixable + 1))
      return 0
    fi
    git diff --quiet "$FORK_POINT" -- "$f" && return 0
    if [ "$mode" = check ]; then
      echo "copyright-headers: $f changes an upstream file but lacks \"Modifications Copyright <year> Cru\"" >&2
      problems=$((problems + 1))
    else
      add_modifications_line "$f"
      echo "copyright-headers: $f: added Modifications Copyright line" >&2
    fi
    return 0
  fi

  local p
  p=$(comment_prefix "$f")
  if [ -n "$p" ] && ! in_fork_point "$f" && ! head -n 5 "$f" | grep -q 'Copyright'; then
    if [ "$mode" = check ]; then
      echo "copyright-headers: $f is a new file without a copyright header" >&2
      problems=$((problems + 1))
    else
      add_cru_header "$f" "$p"
      echo "copyright-headers: $f: added Cru copyright header" >&2
    fi
  fi
}

files=()
if $hook; then
  path=$(jq -r '.tool_input.file_path // empty')
  [ -n "$path" ] && files+=("$path")
elif [ $# -gt 0 ]; then
  files=("$@")
else
  while IFS= read -r f; do files+=("$f"); done < <(
    git diff --name-only --diff-filter=AMR "$FORK_POINT"
    git ls-files --others --exclude-standard
  )
fi

for f in "${files[@]+"${files[@]}"}"; do
  rel=$(repo_path "$f")
  [ -n "$rel" ] && process "$rel"
done

if [ "$unfixable" -gt 0 ]; then
  $hook && exit 2
  exit 1
fi
if [ "$problems" -gt 0 ]; then
  echo "copyright-headers: run scripts/copyright-headers.sh to fix" >&2
  exit 1
fi
exit 0
