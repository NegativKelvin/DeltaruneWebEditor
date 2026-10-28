#!/usr/bin/env bash
# Builds the pinned UndertaleModCli (GPL-3.0) from source as a self-contained .NET app.
# Usage: scripts/build-utmt.sh <rid> <out dir> [source dir]
#   rid: win-x64 | linux-x64 | osx-arm64 | osx-x64
# Needs git and the .NET 10 SDK. Writes the CLI, its LICENSE.txt, and a source archive (for GPL-3.0 section 6) to <out dir>.
set -euo pipefail

UTMT_TAG=0.9.2.0
UTMT_COMMIT=03c7048438347799e0717b6e57a9358d000472e0
UTMT_REPO=https://github.com/UnderminersTeam/UndertaleModTool

rid=${1:?rid}
out=${2:?out dir}
src=${3:-$(mktemp -d)/utmt-src}

if [ ! -d "$src/.git" ]; then
  git clone --depth 1 --branch "$UTMT_TAG" --recurse-submodules --shallow-submodules "$UTMT_REPO" "$src"
fi
# Physical path: on macOS mktemp gives /var (a symlink to /private/var), and the mixed paths make MSBuild flatten content files.
src=$(cd "$src" && pwd -P)
got=$(git -C "$src" rev-parse HEAD)
if [ "$got" != "$UTMT_COMMIT" ]; then
  echo "UTMT tag $UTMT_TAG is at $got, expected $UTMT_COMMIT" >&2
  exit 1
fi

mkdir -p "$out"
# Corresponding source: every tracked file of the built commit, submodules included.
git -C "$src" ls-files -z --recurse-submodules \
  | tar -cz -C "$src" --null -T - -f - > "$out/UndertaleModTool-$UTMT_TAG-src.tar.gz"
dotnet publish "$src/UndertaleModCli" -c Release -r "$rid" --self-contained true \
  -p:PublishSingleFile=false -p:DebugType=none -o "$out/UndertaleModCli"
cp "$src/LICENSE.txt" "$out/UndertaleModCli/LICENSE.txt"

echo "built UndertaleModCli $UTMT_TAG ($rid) in $out/UndertaleModCli"
