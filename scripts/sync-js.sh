#!/bin/sh
# Copies the kit's page-side JS twins (js/*.js) into a project, or checks
# that the project's copies are current.
#
#   scripts/sync-js.sh <project-dir> [--check] [--dest <dir>]
#
#   <project-dir>  the project's root (e.g. ../templates/simple-multithreaded-c)
#   --dest <dir>   where the files go, relative to it (default: dom/runtime/kit)
#   --check        change nothing; exit 1 if any copy is missing or differs
#                  (for CI: run it with the kit at the version the project's
#                  Cargo.toml pins)
#
# Each copy starts with a header naming the kit version (Cargo.toml's
# workspace version), so a project shows which kit its page runs. Edit the
# kit's js/, never the copies.
set -eu

kit_dir=$(cd "$(dirname "$0")/.." && pwd)
project=""
dest="dom/runtime/kit"
check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check=1 ;;
    --dest) shift; dest="${1:?--dest needs a directory}" ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) if [ -z "$project" ]; then project="$1"; else echo "sync-js: unexpected argument: $1" >&2; exit 2; fi ;;
  esac
  shift
done
[ -n "$project" ] || { echo "usage: scripts/sync-js.sh <project-dir> [--check] [--dest <dir>]" >&2; exit 2; }
[ -d "$project" ] || { echo "sync-js: no such directory: $project" >&2; exit 2; }

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"$/\1/p;}' "$kit_dir/Cargo.toml")
[ -n "$version" ] || { echo "sync-js: no workspace version in $kit_dir/Cargo.toml" >&2; exit 2; }

target="$project/$dest"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

stale=0
for source in "$kit_dir"/js/*.js; do
  name=$(basename "$source")
  {
    printf '%s\n' "// awsm-renderer-kit v$version: js/$name, copied by the kit's scripts/sync-js.sh."
    printf '%s\n' "// Do not edit this copy: change the kit, then sync again."
    printf '%s\n' "//"
    cat "$source"
  } > "$tmp/$name"
  if [ "$check" -eq 1 ]; then
    if ! cmp -s "$tmp/$name" "$target/$name"; then
      echo "sync-js: $dest/$name is missing or not kit v$version's" >&2
      stale=1
    fi
  else
    mkdir -p "$target"
    cp "$tmp/$name" "$target/$name"
    echo "synced $dest/$name (kit v$version)"
  fi
done

if [ "$check" -eq 1 ]; then
  if [ "$stale" -ne 0 ]; then
    echo "sync-js: run scripts/sync-js.sh $project to update" >&2
    exit 1
  fi
  echo "sync-js: $dest is kit v$version"
fi
