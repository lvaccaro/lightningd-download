#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(feature = "doc", cfg_attr(all(), doc = include_str!("../README.md")))]

mod versions;

use anyhow::Context;
use log::{debug, error, warn};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;
use std::{env, fmt, fs, thread};
use tempfile::TempDir;
use clightningrpc::LightningRPC;

pub use anyhow;
pub use tempfile;
//pub use which;

#[derive(Debug)]
/// Struct representing the bitcoind process with related information
pub struct LightningD {
    /// Process child handle, used to terminate the process when this struct is dropped
    process: Child,
    /// Rpc client linked to this bitcoind process
    pub client: LightningRPC,
    /// Work directory, where the node store blocks and other stuff.
    work_dir: DataDir,
}

#[derive(Debug)]
/// The DataDir struct defining the kind of data directory the node
/// will contain. Data directory can be either persistent, or temporary.
pub enum DataDir {
    /// Persistent Data Directory
    Persistent(PathBuf),
    /// Temporary Data Directory
    Temporary(TempDir),
}

impl DataDir {
    /// Return the data directory path
    fn path(&self) -> PathBuf {
        match self {
            Self::Persistent(path) => path.to_owned(),
            Self::Temporary(tmp_dir) => tmp_dir.path().to_path_buf(),
        }
    }
}

/// All the possible error in this crate
pub enum Error {
    /// Wrapper of io Error
    Io(std::io::Error),
    /// Wrapper of rpc Error
    Rpc(),
    /// Returned when calling methods requiring a feature to be activated, but it's not
    NoFeature,
    /// Returned when calling methods requiring a env var to exist, but it's not
    NoEnvVar,
    /// Returned when calling methods requiring the lightningd executable but none is found
    /// (no feature, no `LIGHTNINGD_EXE`, no `lightningd` in `PATH` )
    NoLightningdExecutableFound,
    /// Wrapper of early exit status
    EarlyExit(ExitStatus),
    /// Returned when both tmpdir and staticdir is specified in `Conf` options
    BothDirsSpecified,
    /// Returned when -rpcuser and/or -rpcpassword is used in `Conf` args
    /// It will soon be deprecated, please use -rpcauth instead
    RpcUserAndPasswordUsed,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(_) => write!(f, "io::Error"),
            Error::Rpc() => write!(f, "rpc::Error"),
            Error::NoFeature => write!(f, "Called a method requiring a feature to be set, but it's not"),
            Error::NoEnvVar => write!(f, "Called a method requiring env var `LIGHTNINGD_EXE` to be set, but it's not"),
            Error::NoLightningdExecutableFound =>  write!(f, "`lightningd` executable is required, provide it with one of the following: set env var `LIGHTNINGD_EXE` or use a feature like \"22_1\" or have `lightningd` executable in the `PATH`"),
            Error::EarlyExit(e) => write!(f, "The lightningd process terminated early with exit code {}", e),
            Error::BothDirsSpecified => write!(f, "tempdir and staticdir cannot be enabled at same time in configuration options"),
            Error::RpcUserAndPasswordUsed => write!(f, "`-rpcuser` and `-rpcpassword` cannot be used, it will be deprecated soon and it's recommended to use `-rpcauth` instead which works alongside with the default cookie authentication")
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

const INVALID_ARGS: [&str; 2] = ["-rpcuser", "-rpcpassword"];

/// The node configuration parameters, implements a convenient [Default] for most common use.
///
/// `#[non_exhaustive]` allows adding new parameters without breaking downstream users.
/// Users cannot instantiate the struct directly, they need to create it via the `default()` method
/// and mutate fields according to their preference.
///
/// Default values:
/// ```
/// let mut conf = lightningd::Conf::default();
/// conf.args = vec!["--regtest"];
/// conf.view_stdout = false;
/// conf.network = "regtest";
/// conf.tmpdir = None;
/// conf.staticdir = None;
/// conf.attempts = 3;
/// assert_eq!(conf, lightningd::Conf::default());
/// ```
///
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Conf<'a> {
    /// Lightningd command line arguments containing no spaces like `vec!["-dbcache=300", "-regtest"]`
    /// note that `port`, `rpcport`, `connect`, `datadir`, `listen`
    /// cannot be used because they are automatically initialized.
    pub args: Vec<&'a str>,

    /// if `true` lightning log output will not be suppressed
    pub view_stdout: bool,

    /// Must match what specified in args without dashes, needed to locate the cookie file
    /// directory with different/esoteric networks
    pub network: &'a str,

    /// Optionally specify a temporary or persistent working directory for the node.
    /// The following two parameters can be configured to simulate desired working directory configuration.
    ///
    /// tmpdir is Some() && staticdir is Some() : Error. Cannot be enabled at same time.
    /// tmpdir is Some(temp_path) && staticdir is None : Create temporary directory at `tmpdir` path.
    /// tmpdir is None && staticdir is Some(work_path) : Create persistent directory at `staticdir` path.
    /// tmpdir is None && staticdir is None: Creates a temporary directory in OS default temporary directory (eg /tmp) or `TEMPDIR_ROOT` env variable path.
    ///
    /// It may be useful for example to set to a ramdisk via `TEMPDIR_ROOT` env option so that
    /// lightning nodes spawn very fast because their datadirs are in RAM. Should not be enabled with persistent
    /// mode, as it cause memory overflows.

    /// Temporary directory path
    pub tmpdir: Option<PathBuf>,

    /// Persistent directory path
    pub staticdir: Option<PathBuf>,

    /// Try to spawn the process `attempt` time
    ///
    /// The OS is giving available ports to use, however, they aren't booked, so it could rarely
    /// happen they are used at the time the process is spawn. When retrying other available ports
    /// are returned reducing the probability of conflicts to negligible.
    pub attempts: u8,
}

impl Default for Conf<'_> {
    fn default() -> Self {
        Conf {
            args: vec!["--regtest"],
            view_stdout: false,
            network: "regtest",
            tmpdir: None,
            staticdir: None,
            attempts: 3,
        }
    }
}

impl LightningD {
    /// Launch the lightningd process from the given `exe` executable with default args.
    ///
    /// Waits for the node to be ready to accept connections before returning
    pub fn new<S: AsRef<OsStr>>(exe: S) -> anyhow::Result<LightningD> {
        LightningD::with_conf(exe, &Conf::default())
    }

    /// Launch the lightningd process from the given `exe` executable with given [Conf] param
    pub fn with_conf<S: AsRef<OsStr>>(exe: S, conf: &Conf) -> anyhow::Result<LightningD> {
        let tmpdir = conf
            .tmpdir
            .clone()
            .or_else(|| env::var("TEMPDIR_ROOT").map(PathBuf::from).ok());
        let work_dir = match (&tmpdir, &conf.staticdir) {
            (Some(_), Some(_)) => return Err(Error::BothDirsSpecified.into()),
            (Some(tmpdir), None) => DataDir::Temporary(TempDir::new_in(tmpdir)?),
            (None, Some(workdir)) => {
                fs::create_dir_all(workdir)?;
                DataDir::Persistent(workdir.to_owned())
            }
            (None, None) => DataDir::Temporary(TempDir::new()?),
        };

        let work_dir_path = work_dir.path();
        debug!("work_dir: {:?}", work_dir_path);
        /*let cookie_file = work_dir_path.join(conf.network).join(".cookie");
        let rpc_port = get_available_port()?;
        let rpc_socket = SocketAddrV4::new(LOCAL_IP, rpc_port);
        let rpc_url = format!("http://{}", rpc_socket);
        let (p2p_args, p2p_socket) = match conf.p2p {
            P2P::No => (vec!["-listen=0".to_string()], None),
            P2P::Yes => {
                let p2p_port = get_available_port()?;
                let p2p_socket = SocketAddrV4::new(LOCAL_IP, p2p_port);
                let p2p_arg = format!("-port={}", p2p_port);
                let args = vec![p2p_arg];
                (args, Some(p2p_socket))
            }
            P2P::Connect(other_node_url, listen) => {
                let p2p_port = get_available_port()?;
                let p2p_socket = SocketAddrV4::new(LOCAL_IP, p2p_port);
                let p2p_arg = format!("-port={}", p2p_port);
                let connect = format!("-connect={}", other_node_url);
                let mut args = vec![p2p_arg, connect];
                if listen {
                    args.push("-listen=1".to_string())
                }
                (args, Some(p2p_socket))
            }
        };*/
        let stdout = if conf.view_stdout {
            Stdio::inherit()
        } else {
            Stdio::null()
        };

        let datadir_arg = format!("--lightning-dir={}", work_dir_path.display());
        //let rpc_arg = format!("-rpcport={}", rpc_port);
        let default_args = [&datadir_arg];
        let conf_args = validate_args(conf.args.clone())?;

        debug!(
            "launching {:?} with args: {:?} AND custom args: {:?}",
            exe.as_ref(),
            default_args,
            conf_args
        );

        let mut process = Command::new(exe.as_ref())
            .args(&default_args)
            .args(&conf_args)
            .stdout(stdout)
            .spawn()
            .with_context(|| format!("Error while executing {:?}", exe.as_ref()))?;

        //let node_url_default = format!("{}/wallet/default", rpc_url);
        let mut i = 0;
        // wait lightnings is ready, use default wallet
        let client = loop {
            if let Some(status) = process.try_wait()? {
                if conf.attempts > 0 {
                    warn!("early exit with: {:?}. Trying to launch again ({} attempts remaining), maybe some other process used our available port", status, conf.attempts);
                    let mut conf = conf.clone();
                    conf.attempts -= 1;
                    return Self::with_conf(exe, &conf)
                        .with_context(|| format!("Remaining attempts {}", conf.attempts));
                } else {
                    error!("early exit with: {:?}", status);
                    return Err(Error::EarlyExit(status).into());
                }
            }
            thread::sleep(Duration::from_millis(100));
            assert!(process.stderr.is_none());
            let sock: PathBuf = work_dir_path.join(conf.network).join("lightning-rpc");
            println!("{}", sock.as_path().to_str().unwrap());
            let client_result = LightningRPC::new(sock);
            if client_result.getinfo().is_ok() {
                break client_result
            }
            debug!(
                "lightning client for process {} not ready ({})",
                process.id(),
                i
            );

            i += 1;
        };

        Ok(LightningD {
            process,
            client,
            work_dir,
        })
    }

    /// Return the current workdir path of the running node
    pub fn workdir(&self) -> PathBuf {
        self.work_dir.path()
    }

    /// Stop the node, waiting correct process termination
    pub fn stop(&mut self) -> anyhow::Result<ExitStatus> {
        self.client.stop()?;
        Ok(self.process.wait()?)
    }
}

#[cfg(feature = "download")]
impl LightningD {
    /// create LightningD struct with the downloaded executable.
    pub fn from_downloaded() -> anyhow::Result<LightningD> {
        LightningD::new(downloaded_exe_path()?)
    }
    /// create LightningD struct with the downloaded executable and given Conf.
    pub fn from_downloaded_with_conf(conf: &Conf) -> anyhow::Result<LightningD> {
        LightningD::with_conf(downloaded_exe_path()?, conf)
    }
}

impl Drop for LightningD {
    fn drop(&mut self) {
        if let DataDir::Persistent(_) = self.work_dir {
            let _ = self.stop();
        }
        let _ = self.process.kill();
    }
}


/// Provide the bitcoind executable path if a version feature has been specified
#[cfg(not(feature = "download"))]
pub fn downloaded_exe_path() -> anyhow::Result<String> {
    Err(Error::NoFeature.into())
}

/// Provide the lightningd executable path if a version feature has been specified
#[cfg(feature = "download")]
pub fn downloaded_exe_path() -> anyhow::Result<String> {
    let mut path: PathBuf = env!("OUT_DIR").into();
    path.push("lightning");
    path.push("usr");
    path.push("bin");
    path.push("lightningd");

    Ok(format!("{}", path.display()))
}
/// Returns the daemon `lightningd` executable with the following precedence:
///
/// 1) If it's specified in the `LIGHTNINGD_EXE` env var
/// 2) If there is no env var but an auto-download feature such as `23_1` is enabled, returns the
/// path of the downloaded executabled
/// 3) If neither of the precedent are available, the `lightningd` executable is searched in the `PATH`
pub fn exe_path() -> anyhow::Result<String> {
    if let Ok(path) = std::env::var("LIGHTNINGD_EXE") {
        return Ok(path);
    }
    if let Ok(path) = downloaded_exe_path() {
        return Ok(path);
    }
    which::which("lightningd")
        .map_err(|_| Error::NoLightningdExecutableFound.into())
        .map(|p| p.display().to_string())
}

/// Validate the specified arg if there is any unavailable or deprecated one
pub fn validate_args(args: Vec<&str>) -> anyhow::Result<Vec<&str>> {
    args.iter().try_for_each(|arg| {
        // other kind of invalid arguments can be added into the list if needed
        if INVALID_ARGS.iter().any(|x| arg.starts_with(x)) {
            return Err(Error::RpcUserAndPasswordUsed);
        }
        Ok(())
    })?;

    Ok(args)
}


#[cfg(test)]
mod test {
    use crate::exe_path;
    use crate::LightningD;

    fn init() -> String {
        let _ = env_logger::try_init();
        exe_path().unwrap()
    }

    #[test]
    fn test_lightningd() {
        let exe = init();
        let lightningd = LightningD::new(exe).unwrap();
        let info = lightningd.client.getinfo().unwrap();
        println!("{:?}", info);
    }
}

