use std::process::Command;
use std::env;
use std::path::Path;
use std::vec::Vec;

use {Config, CliError, LalResult};

/// Verifies that `id -u` and `id -g` are both 1000
///
/// Docker user namespaces are not properly supported by our setup,
/// so for builds to work sanely, user ids and group ids should match a standard
/// linux setup and in particular, match the first user in a normal container.
fn permission_sanity_check() -> LalResult<()> {
    let uid_output = try!(Command::new("id").arg("-u").output());
    let uid_str = String::from_utf8_lossy(&uid_output.stdout);
    let uid = uid_str.trim().parse::<u32>().unwrap(); // trust `id -u` is sane
    if uid != 1000 {
        return Err(CliError::DockerPermissionSafety(format!("UID is {}, not 1000", uid)));
    }

    let gid_output = try!(Command::new("id").arg("-g").output());
    let gid_str = String::from_utf8_lossy(&gid_output.stdout);
    let gid = gid_str.trim().parse::<u32>().unwrap(); // trust `id -g` is sane
    if gid != 1000 {
        return Err(CliError::DockerPermissionSafety(format!("GID is {}, not 1000", gid)));
    }

    Ok(())
}


/// Runs an arbitrary command in the configured docker environment
///
/// This will mount the current directory as `~/volume` as well as a few conveniences,
/// and absorb the `Stdio` supplied by this `Command`.
///
/// This is the most general function, used by both `lal build` and `lal shell`.
pub fn docker_run(cfg: &Config,
                  command: Vec<String>,
                  interactive: bool,
                  printonly: bool)
                  -> LalResult<()> {
    trace!("Finding home and cwd");
    let home = env::home_dir().unwrap(); // crash if no $HOME
    let git_cfg = Path::new(&home).join(".gitconfig");
    let pwd = env::current_dir().unwrap();

    // construct arguments vector
    let mut args: Vec<String> = vec!["run".into(), "--rm".into()];
    for mount in cfg.mounts.clone() {
        trace!(" - mounting {}", mount.src);
        args.push("-v".into());
        let mnt = format!("{}:{}{}",
                          mount.src,
                          mount.dest,
                          if mount.readonly { ":ro" } else { "" });
        args.push(mnt);
    }
    trace!(" - mounting {}", git_cfg.display());
    trace!(" - mounting {}", pwd.display());
    args.push("-v".into());
    args.push(format!("{}:/home/lal/.gitconfig:ro", git_cfg.display()));
    args.push("-v".into());
    args.push(format!("{}:/home/lal/volume", pwd.display()));

    args.push("-w".into());
    args.push("/home/lal/volume".into());
    args.push("--user".into());
    args.push("lal".into());
    args.push(format!("{}", if interactive { "-it" } else { "-t" }));
    args.push(cfg.container.clone());
    for c in command {
        args.push(c);
    }

    // run or print docker command
    if printonly {
        println!("docker {}", args.join(" "));
    } else {
        trace!("Performing docker permission sanity check");
        let _ = permission_sanity_check().map_err(|e| {
            warn!("{}", e);
            warn!("You will likely have permission issues");
        }); // keep going, but with a warning if it failed
        trace!("Permissions verified, entering docker");
        let s = try!(Command::new("docker").args(&args).status());
        trace!("Exited docker");
        if !s.success() {
            return Err(CliError::SubprocessFailure(s.code().unwrap_or(1001)));
        }
    }
    Ok(())
}

/// Mounts and enters `.` in an interactive bash shell using the configured container.
pub fn shell(cfg: &Config, printonly: bool, cmd: Option<&str>) -> LalResult<()> {
    if !printonly {
        info!("Entering docker container");
    }
    let mut bash = vec!["/bin/bash".into()];
    if cmd.is_some() {
        bash.push("-c".into());
        // Need to double quote apparently
        bash.push(format!("\"\"{}\"\"", cmd.unwrap()));
    }
    docker_run(&cfg, bash, true, printonly)
}
