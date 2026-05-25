use super::*;

pub(super) struct ForgeServerLaunch {
    pub(super) artifact: PathBuf,
    pub(super) args: Vec<String>,
}

pub(super) fn prepare_forge_server_launch(
    config: &Config,
    instance: &Instance,
    root: &Path,
    dirs: &StateDirs,
    game_dir: &Path,
    java: &Path,
) -> Result<Option<ForgeServerLaunch>, String> {
    if !matches!(instance.loader.as_str(), "forge" | "neoforge") {
        return Ok(None);
    }

    let Some(installer_maven) = locked_value(root, &instance.name, "installer_maven")? else {
        return Ok(None);
    };
    let coordinates = MavenCoordinates::parse_coordinate(&installer_maven)
        .ok_or_else(|| format!("invalid Forge installer coordinate `{installer_maven}`"))?;
    let loader_args = installer_server_args_file(&instance.loader, coordinates.version);
    let loader_args_path = game_dir.join(loader_args.trim_start_matches('@'));
    if loader_args_path.is_file() {
        return Ok(Some(ForgeServerLaunch {
            artifact: loader_args_path,
            args: forge_server_launch_args(game_dir, loader_args),
        }));
    }

    let repositories = repositories_with_builtins(&config.repositories);
    let installer = resolve_maven_artifact(
        &repositories,
        &coordinates,
        &dirs.cache.join("downloads").join("maven"),
    )?
    .ok_or_else(|| format!("failed to resolve Forge installer `{installer_maven}`"))?;

    let mut installer_command = Command::new(java);
    installer_command
        .arg("-jar")
        .arg(&installer.path)
        .arg("--installServer")
        .arg(".")
        .current_dir(game_dir);
    let installer_output = run_process_with_timeout(&mut installer_command, None)
        .map_err(|error| format!("failed to run Forge installer {}: {error}", java.display()))?;
    print!("{}", String::from_utf8_lossy(&installer_output.stdout));
    eprint!("{}", String::from_utf8_lossy(&installer_output.stderr));

    if !installer_output.status.success() {
        return Err(format!(
            "Forge installer failed with exit code {}",
            installer_output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }

    if !loader_args_path.is_file() {
        return Err(format!(
            "Forge installer did not create {}",
            loader_args_path.display()
        ));
    }

    Ok(Some(ForgeServerLaunch {
        artifact: installer.path,
        args: forge_server_launch_args(game_dir, loader_args),
    }))
}

fn forge_server_launch_args(game_dir: &Path, loader_args: String) -> Vec<String> {
    let mut args = Vec::new();
    if game_dir.join("user_jvm_args.txt").is_file() {
        args.push("@user_jvm_args.txt".to_string());
    }
    args.push(loader_args);
    args.push("nogui".to_string());
    args
}

fn installer_server_args_file(loader: &str, version: &str) -> String {
    let (group_path, artifact) = match loader {
        "neoforge" => ("net/neoforged", "neoforge"),
        _ => ("net/minecraftforge", "forge"),
    };
    let file_name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    format!("@libraries/{group_path}/{artifact}/{version}/{file_name}")
}
