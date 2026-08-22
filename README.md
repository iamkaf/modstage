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
  <a href="#using-modstage">Using Modstage</a> ·
  <a href="#why">Why</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#developing-modstage">Developing Modstage</a>
</p>

---

Modstage stages and launches real Minecraft from a reproducible project file. Use it when you want to test the same published mod jars your users install, outside Gradle, IDE run configs, and GUI launcher state.

The launcher is headless. Minecraft is not.

## Using Modstage

This section is for mod developers who want to run Modstage against their own mod jars.

### Quick Start

Create a config in your mod project, resolve it, then run one side:

```bash
cd /path/to/your-mod
modstage init # Creates the modstage.toml file, edit it to your preferences
modstage run client example-fabric-26.1.2 # Runs a Minecraft instance
```

Edit `modstage.toml` before resolving:

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
loader_version = "latest"
sides = ["client", "server"]
mods = [
  "maven:com.example:example-fabric:1.0.0+26.1.2",
  "modrinth:fabric-api:latest",
  "./local-tooling.jar",
]
```

Run from anywhere with an explicit config:

```bash
modstage --config /path/to/example/modstage.toml run server example-fabric-26.1.2
```

### Why

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

### How It Works

```text
modstage.toml
  -> resolve Mojang, loader, library, asset index, and mod metadata
  -> write the per-instance state lock
  -> restore locked artifacts into cache
  -> reconcile durable instance state
  -> launch Java
  -> write logs, launch plan, crash reports, and run.toml
```

Edit `modstage.toml` to your preferences.

### Configuration

The starter config above is enough for a normal Fabric client/server check. Add more `[[instance]]` blocks for Forge, NeoForge, server-only checks, or vanilla baselines.

| Field | Description |
| --- | --- |
| `[project].name` | Project name used in state paths |
| `[repositories]` | Ordered Maven repositories for Maven mod sources |
| `[[instance]].name` | Stable instance id for `resolve`, `run`, and `inspect` |
| `minecraft` | Mojang Minecraft version |
| `loader` | `vanilla`, `fabric`, `forge`, or `neoforge` |
| `loader_version` | Exact loader version, or `latest` to resolve the newest loader for `minecraft` |
| `sides` | `client`, `server`, or both |
| `modrinth_pack` | Optional pinned Modrinth `.mrpack` source, including side rules and overrides |
| `mods` | Modrinth ids, Maven single-jar coordinates, or local jars |
| `server_properties` | Optional inline table of deliberate server-only property overrides |

Repository fallback is normal ordered Maven fallback. The first repository that contains an artifact wins.

Use `modrinth:project:version` for a specific Modrinth version number or version id. Use `modrinth:project:latest`, or omit the version, to resolve the newest Modrinth version matching the instance loader and Minecraft version.

Set `modrinth_pack = "modrinth:cobbleverse:1.7.42"` on an instance to use a
published pack as its base. Modstage validates the pack's Minecraft and loader
versions, stages required and optional files according to their client/server
environment, applies common and side-specific overrides, and records every
resolved path and SHA-256 in the lock. Entries in `mods` are then layered on top
for the mod under test and any deliberate additions. Required Modrinth
dependencies are resolved recursively; dependencies already supplied by the
pack are deduplicated.

Local production reproductions commonly need an offline test identity. Express
that deviation in the instance instead of editing staged state:

```toml
server_properties = { online-mode = "false", enforce-secure-profile = "false" }
```

Modstage applies these values after pack overrides and fixtures and records them
in the instance lock.

`latest` metadata is cached briefly, then refreshed. Lockfiles keep the exact resolved versions.

### Commands

| Command | Description |
| --- | --- |
| `modstage init` | Create a starter `modstage.toml` |
| `modstage resolve [instance]` | Generate per-instance state lockfiles without launching |
| `modstage run <client\|server> <instance>` | Resolve if the lock is missing or the instance config changed, then stage and launch one side |
| `modstage inspect config` | Print config and state directories |
| `modstage inspect lock [instance]` | Print generated state lockfiles |
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

`run` options:

| Option | Description |
| --- | --- |
| `--locked` | Fail instead of refreshing a missing or stale instance lock |
| `--keep-alive` | Leave a ready server running until timeout |
| `--java <path>` | Use an explicit Java executable |
| `--timeout <duration>` | Bound the launch, for example `120s` |

### Runtime Behavior

Modstage keeps state in two places:

| Platform | Durable Data | Redownloadable Cache |
| --- | --- | --- |
| Linux | `~/.local/share/modstage` | `~/.cache/modstage` |
| macOS | `~/Library/Application Support/modstage` | `~/Library/Caches/modstage` |
| Windows | `%APPDATA%\modstage` | `%LOCALAPPDATA%\modstage\Cache` |

`MODSTAGE_DATA_HOME` and `MODSTAGE_CACHE_HOME` override the platform base
directories for isolated automation and disposable environments.

Project state is scoped by project name plus a hash of the config root. Two projects with the same name do not collide.

### Client Runs

Client runs launch the real graphical Minecraft client and follow normal process exit or timeout behavior.

### Server Runs

Server runs watch for the standard Minecraft ready line, send `stop`, and accept confirmed shutdown as a pass. Server instances write `eula.txt=true` automatically.

An external test orchestrator can pass `--keep-alive` to retain ownership after
readiness. The run remains bounded by `--timeout` and still captures the normal
Modstage report; the orchestrator is responsible for requesting graceful
shutdown before that deadline.

### Forge And NeoForge

Forge and NeoForge use installer metadata. Modstage reads installer profiles, resolves processor classpaths, expands launcher placeholders, runs client processors when needed, and uses installer-generated server argfiles for server launches.

### Reports

Every run writes a report directory under the project run root.

| File | Contents |
| --- | --- |
| `run.toml` | Instance, side, status, Java path, artifact path, timeout, exit code, and failure class |
| `launch-plan.toml` | Java command arguments |
| `stdout.log` | Raw process stdout |
| `stderr.log` | Raw process stderr |
| `minecraft-latest.log` | Copied Minecraft log when present |
| `crash-reports/*` | Copied crash reports when present |

Failure classes include timeout, crash report, mixin failure, Minecraft startup failure, and process failure.

### Downloads

Modstage records and verifies Mojang's asset index, then downloads the objects it names into the asset cache so a client can actually start. Object hashes stay out of the lockfile. A later `--locked` run restores any missing objects from the locked index.

Downloads use `reqwest` with Rustls.

---

## Developing Modstage

This section is for people changing Modstage itself.

| Task | Command |
| --- | --- |
| Build debug binary | `cargo build` |
| Build release binary | `cargo build --release` |
| Run checks | `cargo check` |
| Run lint checks | `cargo clippy --all-targets -- -D warnings` |
| Run tests | `cargo test` |
| Audit dependencies | `cargo audit --deny warnings` |
| Check formatting | `cargo fmt -- --check` |
| Format code | `cargo fmt` |

Keep user-facing behavior covered by integration tests under `tests/`. Runtime behavior should use bounded fake Java or bounded launcher runs so test jobs cannot hang.

Automation can use normal bounded runs; Modstage resolves the selected instance before launching when needed:

```bash
modstage run client example-fabric-26.1.2 --timeout 120s
```

Use `--locked` only when a pre-existing state lock is required and missing or stale locks should fail immediately:

```bash
modstage run client example-fabric-26.1.2 --locked --timeout 120s
```

### CI And Releases

| Workflow | Trigger | Behavior |
| --- | --- | --- |
| `check.yml` | `push`, `pull_request` | Runs all Rust test targets on Linux and Windows |
| `release.yml` | Manual `workflow_dispatch` | Tests, builds release binary, uploads `modstage-linux-x86_64.tar.gz` |

Release publishing is manual. Linux is the only release target for now.

## Acknowledgements

Modstage builds on the public Minecraft launcher ecosystem and the behavior documented by Mojang, Fabric, Forge, NeoForge, Maven, and Modrinth. The project also owes design context to existing open source launchers, especially Modrinth App and Prism Launcher.

## License

Modstage is licensed under the GNU General Public License, Version 3 only. See [LICENSE](LICENSE).
