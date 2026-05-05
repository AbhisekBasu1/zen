use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod darwin;

#[cfg(target_os = "macos")]
pub use darwin::{Child, Command, Stdio};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000_u32;

pub fn new_command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

pub fn find_executable(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    find_executable_in_paths(
        program,
        std::env::var_os("PATH")?,
        std::env::current_dir().ok()?,
    )
}

pub fn find_global_executable(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    find_executable(program)
}

pub fn find_executable_in_paths(
    program: impl AsRef<OsStr>,
    search_paths: impl AsRef<OsStr>,
    current_dir: impl AsRef<Path>,
) -> Option<PathBuf> {
    let program = Path::new(program.as_ref());
    if program.components().count() > 1 {
        let candidate = if program.is_absolute() {
            program.to_path_buf()
        } else {
            current_dir.as_ref().join(program)
        };
        return is_executable_file(&candidate).then_some(candidate);
    }

    std::env::split_paths(search_paths.as_ref()).find_map(|dir| {
        executable_candidates(&dir.join(program))
            .into_iter()
            .find(|candidate| is_executable_file(candidate))
    })
}

#[cfg(unix)]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    vec![path.to_path_buf()]
}

#[cfg(windows)]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return vec![path.to_path_buf()];
    }

    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    pathext
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            let mut path = path.as_os_str().to_os_string();
            path.push(extension);
            PathBuf::from(path)
        })
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    vec![path.to_path_buf()]
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(not(any(unix, windows)))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(target_os = "windows")]
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
pub fn new_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    std::process::Command::new(program)
}

#[cfg(not(target_os = "macos"))]
pub type Child = smol::process::Child;

#[cfg(not(target_os = "macos"))]
pub use std::process::Stdio;

#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
pub struct Command(smol::process::Command);

#[cfg(not(target_os = "macos"))]
impl Command {
    #[inline]
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        #[cfg(target_os = "windows")]
        {
            use smol::process::windows::CommandExt;
            let mut cmd = smol::process::Command::new(program);
            cmd.creation_flags(CREATE_NO_WINDOW);
            Self(cmd)
        }
        #[cfg(not(target_os = "windows"))]
        Self(smol::process::Command::new(program))
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.0.args(args);
        self
    }

    pub fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.0.get_args()
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.0.env(key, val);
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.envs(vars);
        self
    }

    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.0.env_remove(key);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.0.env_clear();
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdin(cfg.into());
        self
    }

    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdout(cfg.into());
        self
    }

    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stderr(cfg.into());
        self
    }

    pub fn kill_on_drop(&mut self, kill_on_drop: bool) -> &mut Self {
        self.0.kill_on_drop(kill_on_drop);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<Child> {
        self.0.spawn()
    }

    pub async fn output(&mut self) -> std::io::Result<std::process::Output> {
        self.0.output().await
    }

    pub async fn status(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.0.status().await
    }

    pub fn get_program(&self) -> &OsStr {
        self.0.get_program()
    }
}
