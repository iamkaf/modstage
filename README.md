<p align="center">
  <a href="https://github.com/iamkaf/modstage/actions/workflows/check.yml"><img src="https://img.shields.io/github/actions/workflow/status/iamkaf/modstage/check.yml?style=for-the-badge&labelColor=111827&color=38bdf8" alt="Check CI" /></a>
  <img src="https://img.shields.io/badge/rust-2024-f97316?style=for-the-badge&logo=rust&logoColor=f97316&labelColor=111827" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/minecraft-client%20%2B%20server-22c55e?style=for-the-badge&labelColor=111827" alt="Minecraft client and server" />
</p>

<h1 align="center">Modstage</h1>

<p align="center">
  <strong>A headless Minecraft launcher for testing published mod jars in real client and server environments.</strong>
</p>

<p align="center">
  <a href="#why">Why</a> ·
  <a href="#quick-start">Quick Start</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands">Commands</a> ·
  <a href="#runtime-behavior">Runtime Behavior</a>
</p>

---

Modstage stages and launches real Minecraft from a reproducible project file. It is built for mod developers who need to test the jars users actually install, without relying on Gradle run configs, IDE state, or a GUI launcher profile.

The launcher is headless. Minecraft is not.

## Why

Mod development has a gap between "works in Gradle" and "works in a production launcher." Modstage targets that gap.

It checks the failure modes that dev classpaths often hide:

| Risk | What Modstage Does |
| --- | --- |
| Loader wiring | Resolves and launches Fabric, Forge, NeoForge, or vanilla |
| Published artifact drift | Uses published, Maven-local, Modrinth, or local jar inputs |
| Mixin failures | Captures full client/server logs and classpath |
| Resource processing issues | Runs the same client/server jar shape users run |
| Dependency alignment | Resolves Maven libraries through ordered repositories |
| Dirty launcher state | Reconciles `mods/` from the lockfile on each run |

## How It Works

```text
modstage.toml
  -> resolve Mojang, loader, library, asset, and mod metadata
  -> write modstage.lock
  -> restore locked artifacts into cache
  -> reconcile durable instance state
  -> launch Java
  -> write logs, launch plan, crash reports, and run.toml
```

`modstage.toml` is the file humans edit. `modstage.lock` is the generated launch graph.

## Quick Start

Build from source:

```bash
cargo build --release
```

Create a config:

```bash
target/release/modstage init
```

Resolve and run:

```bash
target/release/modstage resolve
target/release/modstage run server my-instance --locked --timeout 120s
```

Run the Liteminer validation target:

```bash
target/release/modstage --config /home/kaf/code/mods/liteminer/modstage.toml resolve
target/release/modstage --config /home/kaf/code/mods/liteminer/modstage.toml run client liteminer-fabric-26.1.2 --locked --timeout 120s
```

## Configuration

Minimal project:

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
  "./local-tooling.jar",
]
```

| Field | Description |
| --- | --- |
| `[project].name` | Project name used in state paths |
| `[repositories]` | Ordered Maven repositories for Maven mod sources |
| `[[instance]].name` | Stable instance id for `resolve`, `run`, and `inspect` |
| `minecraft` | Mojang Minecraft version |
| `loader` | `vanilla`, `fabric`, `forge`, or `neoforge` |
| `loader_version` | Exact loader version, or `latest` where supported |
| `sides` | `client`, `server`, or both |
| `mods` | Direct Modrinth ids, Maven single-jar coordinates, or local jars |

Repository fallback is normal ordered Maven fallback. The first repository that contains an artifact wins.

## Commands

| Command | Description |
| --- | --- |
| `modstage init` | Create a starter `modstage.toml` |
| `modstage resolve [instance]` | Generate `modstage.lock` |
| `modstage run <client\|server> <instance>` | Stage and launch one side of one instance |
| `modstage inspect config` | Print config and state directories |
| `modstage inspect lock [instance]` | Print the lockfile or one instance section |
| `modstage inspect instance <instance> [--side <client\|server>]` | Print staged files |
| `modstage inspect run <run-id>` | Print a saved run report |
| `modstage clean instance <instance> [--side <client\|server>]` | Remove durable staged state |
| `modstage clean cache` | Remove redownloadable cache data |
| `modstage java list` | List discovered and registered Java runtimes |
| `modstage java install <major>` | Register or install a managed Java runtime |
| `modstage java doctor` | Validate Java selection |

Global option:

| Option | Description |
| --- | --- |
| `--config <path>` | Use an explicit `modstage.toml` |

## Runtime Behavior

Modstage keeps state in two places:

| Platform | Durable Data | Redownloadable Cache |
| --- | --- | --- |
| Linux | `~/.local/share/modstage` | `~/.cache/modstage` |
| macOS | `~/Library/Application Support/modstage` | `~/Library/Caches/modstage` |
| Windows | `%APPDATA%\modstage` | `%LOCALAPPDATA%\modstage\Cache` |

Project state is scoped by project name plus a hash of the config root. Two projects with the same name do not collide.

### Client Runs

Client runs launch the real graphical Minecraft client. When TeaKit is installed, Modstage treats the TeaKit readiness log as the bounded success point and stops the process.

### Server Runs

Server runs watch for the standard Minecraft ready line, send `stop`, and accept confirmed shutdown as a pass. Server instances write `eula.txt=true` automatically.

### Forge And NeoForge

Forge and NeoForge use installer metadata. Modstage reads installer profiles, resolves processor classpaths, expands launcher placeholders, runs client processors when needed, and uses installer-generated server argfiles for server launches.

The Forge and NeoForge client setup follows the same broad model as the Modrinth app launcher code:

| Reference | Purpose |
| --- | --- |
| `/home/kaf/code/oss/code/packages/app-lib/src/launcher/mod.rs` | Launcher processor and data model |
| `/home/kaf/code/oss/code/packages/app-lib/src/launcher/args.rs` | Launch argument and placeholder handling |
| `/home/kaf/code/oss/code/packages/app-lib/src/launcher/download.rs` | Minecraft asset and library download model |

## Reports

Every run writes a report directory under the project run root.

| File | Contents |
| --- | --- |
| `run.toml` | Instance, side, status, Java path, artifact path, timeout, exit code, and failure class |
| `launch-plan.toml` | Java command arguments and Modstage environment |
| `stdout.log` | Raw process stdout |
| `stderr.log` | Raw process stderr |
| `minecraft-latest.log` | Copied Minecraft log when present |
| `crash-reports/*` | Copied crash reports when present |

Failure classes include timeout, crash report, mixin failure, Minecraft startup failure, and process failure.

## Downloads

Modstage uses Mojang's normal launcher asset model. Assets come from the asset index and are stored by object hash under `assets/objects/<prefix>/<hash>`.

Downloads use `reqwest` with Rustls. Locked asset restoration is parallel by default.

| Setting | Default | Description |
| --- | ---: | --- |
| `MODSTAGE_DOWNLOAD_CONCURRENCY` | `32` | Worker count for restoring missing locked assets |

## Validation

Current validation target:

```text
/home/kaf/code/mods/liteminer
Minecraft 26.1.2
Liteminer 3.1.0+26.1.2
Fabric, Forge, NeoForge
Java 25
```

Observed local matrix:

| Command | Result |
| --- | --- |
| `run client liteminer-fabric-26.1.2 --locked --timeout 120s` | Passed after TeaKit readiness |
| `run server liteminer-fabric-26.1.2 --locked --timeout 120s` | Passed after ready/stop shutdown |
| `run client liteminer-forge-26.1.2 --locked --timeout 120s` | Passed after Forge client staging and TeaKit readiness |
| `run server liteminer-forge-26.1.2 --locked --timeout 120s` | Passed through Forge installer argfiles |
| `run client liteminer-neoforge-26.1.2 --locked --timeout 120s` | Passed after NeoForge client staging and TeaKit readiness |
| `run server liteminer-neoforge-26.1.2 --locked --timeout 120s` | Passed through NeoForge installer argfiles |

Cold Fabric measurement on the Liteminer target, with no lockfile and no Modstage cache/data:

| Build | Time |
| --- | ---: |
| Serial `curl` downloader | `485.77s` |
| `reqwest` downloader with parallel asset restore | `23.97s` |

## CI And Releases

| Workflow | Trigger | Behavior |
| --- | --- | --- |
| `check.yml` | `push`, `pull_request` | Runs `cargo test` on Linux |
| `release.yml` | Manual `workflow_dispatch` | Tests, builds release binary, uploads `modstage-linux-x86_64.tar.gz` |

Release publishing is manual. Linux is the only release target for now.

## Dependency Policy

Modstage started with no Rust dependencies. It now uses a small explicit set because the launcher needs JSON parsing and a real HTTP client.

| Dependency | Reason |
| --- | --- |
| `serde` | Structured metadata types |
| `serde_json` | Mojang, Modrinth, Fabric, Forge, and NeoForge metadata |
| `reqwest` | HTTP downloads without shelling out to `curl` |

Dependency changes should be justified, pinned through `Cargo.lock`, and checked with `cargo audit`.

## Development

```bash
cargo fmt --check
cargo test
cargo audit
cargo build --release
```

Tests exercise CLI behavior through config files, lockfiles, staged instance state, local HTTP fixtures, fake Java processes, and run reports.

## License

No license file is present yet.
