<p align="center">
  <a href="https://github.com/iamkaf/modstage/actions/workflows/check.yml"><img src="https://img.shields.io/github/actions/workflow/status/iamkaf/modstage/check.yml?style=for-the-badge&labelColor=111827&color=38bdf8" alt="Check CI" /></a>
  <img src="https://img.shields.io/badge/rust-2024-f97316?style=for-the-badge&logo=rust&logoColor=f97316&labelColor=111827" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/dependencies-0-22c55e?style=for-the-badge&labelColor=111827" alt="Zero Rust dependencies" />
</p>

<h1 align="center">Modstage</h1>

<p align="center">
  <strong>A dependency-free Rust CLI that stages and launches real Minecraft mod environments from reproducible config.</strong>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands">Commands</a> ·
  <a href="#validation">Validation</a>
</p>

---

Modstage is a headless Minecraft launcher for mod development. It replaces GUI launcher state and Gradle run configs with `modstage.toml`, a generated TOML lockfile, durable instance directories, full process logs, and run reports.

It answers one question: do these published or Maven-local mod jars work together in the same kind of client or server environment users actually run?

## How It Works

```text
modstage.toml
  -> resolve Mojang + loader + mod artifacts
  -> write modstage.lock
  -> reconcile instance mods/
  -> launch real client or server Java process
  -> save stdout, stderr, Minecraft logs, crash reports, and launch plan
```

Modstage does not fake Minecraft. The launcher is headless; the game process is real.

## Quick Start

```bash
cargo build
cargo run -- init
cargo run -- resolve
cargo run -- run server my-instance --locked --timeout 120s
```

For the current validation target:

```bash
cargo run -- --config /home/kaf/code/mods/liteminer/modstage.toml resolve
cargo run -- --config /home/kaf/code/mods/liteminer/modstage.toml run server liteminer-fabric-26.1.2 --locked --timeout 120s
```

## Configuration

`modstage.toml` is the human-authored project file. `modstage.lock` is generated and should be treated as the resolved launch graph.

```toml
[project]
name = "example"

[repositories]
mavenLocal = "mavenLocal"
kaf = "https://maven.kaf.sh"

[[instance]]
name = "example-fabric-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "0.19.2"
sides = ["client", "server"]
mods = [
  "maven:com.example:example-fabric:1.0.0+26.1.2",
  "modrinth:fabric-api",
  "./local-debug-helper.jar",
]
```

| Field | Purpose |
| --- | --- |
| `[project].name` | Human-readable project name used in state paths |
| `[repositories]` | Ordered Maven repositories used for Maven mod sources |
| `[[instance]].name` | Stable instance name used by `resolve`, `run`, and `inspect` |
| `minecraft` | Mojang Minecraft version |
| `loader` | `vanilla`, `fabric`, `forge`, or `neoforge` |
| `loader_version` | Exact loader version or `latest` where supported |
| `sides` | Any combination of `client` and `server` |
| `mods` | Direct Modrinth, Maven single-jar, or local jar mod sources |

## Commands

| Command | Description |
| --- | --- |
| `modstage init` | Create a starter `modstage.toml` without overwriting an existing file |
| `modstage resolve [instance]` | Resolve Minecraft, loader, library, asset, and mod artifacts into `modstage.lock` |
| `modstage run <client\|server> <instance>` | Stage and launch one side of one instance |
| `modstage inspect config` | Show project config and state directories |
| `modstage inspect lock [instance]` | Print the full lockfile or one instance section |
| `modstage inspect instance <instance> [--side <client\|server>]` | Show staged instance files |
| `modstage inspect run <run-id>` | Show a saved run report |
| `modstage clean instance <instance> [--side <client\|server>]` | Remove staged durable instance state |
| `modstage clean cache` | Remove redownloadable Modstage cache data |
| `modstage java list` | List discovered and managed Java runtimes |
| `modstage java install <major>` | Register or install a managed Java runtime |
| `modstage java doctor` | Validate Java selection |

## Run Reports

Every launch writes a run directory under the project-scoped data root. Reports include:

| Artifact | Description |
| --- | --- |
| `run.toml` | Instance, side, status, Java path, artifact path, timeout, exit code, and failure class |
| `launch-plan.toml` | Java command arguments and Modstage environment variables |
| `stdout.log` | Raw process stdout |
| `stderr.log` | Raw process stderr |
| `latest.log` | Copied Minecraft log when present |
| `crash-reports/*` | Copied crash report when Minecraft produces one |

Server runs watch for the standard Minecraft ready line, send `stop`, and accept a confirmed shutdown as a bounded pass. Client runs launch the real graphical client; in a non-display environment they should fail with a captured crash report or early display error.

## State Layout

Modstage keeps durable instance state separate from redownloadable cache data.

| Platform | Data root | Cache root |
| --- | --- | --- |
| Linux | `~/.local/share/modstage` | `~/.cache/modstage` |
| macOS | `~/Library/Application Support/modstage` | `~/Library/Caches/modstage` |
| Windows | `%APPDATA%\modstage` | `%LOCALAPPDATA%\modstage\Cache` |

Project state is scoped by project name plus a short hash of the config root, so two projects with the same name do not collide.

## Validation

Current target project:

```text
/home/kaf/code/mods/liteminer
Minecraft 26.1.2
Liteminer 3.1.0+26.1.2
Fabric, Forge, NeoForge
Java 25
```

Observed local matrix status:

| Command | Result |
| --- | --- |
| `run server liteminer-fabric-26.1.2 --locked --timeout 120s` | Passed after ready/stop shutdown |
| `run server liteminer-forge-26.1.2 --locked --timeout 120s` | Passed through Forge installer argfiles |
| `run server liteminer-neoforge-26.1.2 --locked --timeout 120s` | Passed through NeoForge installer argfiles |
| `run client liteminer-fabric-26.1.2 --locked --timeout 120s` | Launched real client; failed with GLFW/X11 crash report in this environment |
| `run client liteminer-forge-26.1.2 --locked --timeout 120s` | Launched real client; failed at Forge early display in this environment |
| `run client liteminer-neoforge-26.1.2 --locked --timeout 120s` | Launched real client; failed at NeoForge early display in this environment |

## CI And Releases

| Workflow | Trigger | Behavior |
| --- | --- | --- |
| `check.yml` | `push`, `pull_request` | Runs `cargo test` on Linux |
| `release.yml` | Manual `workflow_dispatch` | Tests, builds `cargo build --release`, and uploads `modstage-linux-x86_64.tar.gz` to a GitHub Release |

Release publishing is manual and Linux-only for now.

## Dependency Policy

Modstage currently has no Rust dependencies. New dependencies should be added only when they remove enough complexity to justify the supply-chain risk, and they must be reviewed before use.

## Development

```bash
cargo test
cargo run -- --help
cargo build --release
```

The implementation is intentionally standard-library-heavy. Tests exercise public CLI behavior through config files, lockfiles, staged instance state, launched fake Java processes, and run reports.

## License

No license file is present yet.
