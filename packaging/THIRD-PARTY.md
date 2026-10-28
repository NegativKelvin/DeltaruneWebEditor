# Third-party software in this package

## UndertaleModTool CLI (`tools/UndertaleModCli/`)

- Project: UndertaleModTool by the Underminers Team, <https://github.com/UnderminersTeam/UndertaleModTool>
- Version: 0.9.2.0 (commit `03c7048438347799e0717b6e57a9358d000472e0`), not modified.
- License: GNU General Public License v3.0. Full text: `tools/UndertaleModCli/LICENSE.txt`.
- Corresponding source: `UndertaleModTool-0.9.2.0-src.tar.gz`, attached to the same GitHub release as this package.
  It holds every file of the built commit, with the Underanalyzer submodule. Build it with
  `scripts/build-utmt.sh` from the Deltarune Web Editor repository.

The CLI is a separate program. `dre-server` only starts it as a child process; it does not link to it.
The Deltarune Web Editor code (`dre-server`, `web/`) stays under its own license.

## .NET runtime (inside `tools/UndertaleModCli/`)

The CLI is published self-contained, so it includes the .NET 10 runtime (MIT license, <https://github.com/dotnet/runtime>)
and its NuGet dependencies (see the licenses of each package on <https://www.nuget.org>).
