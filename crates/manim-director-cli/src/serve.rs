//! `serve` and `open`: bind, print the tokenized workbench URL, serve.

use crate::args::{OpenArgs, ServerArgs};
use anyhow::{anyhow, Result};
use manim_director_engine::{
    EngineMode, Scheduler, SchedulerConfig, ServeConfig, Server, REMOTE_WARNING,
};
use serde_json::json;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// The launcher page outlives the browser's read of it by this much at most.
const LAUNCHER_LIFETIME: Duration = Duration::from_secs(30);

pub async fn serve(root: &Path, args: ServerArgs, machine: bool) -> Result<()> {
    let server = start(root, args, machine).await?;
    server.run().await
}

pub async fn open(root: &Path, args: OpenArgs, machine: bool) -> Result<()> {
    let server = start(root, args.server, machine).await?;
    if !args.no_browser {
        let page = Launcher::write(server.address().port(), &server.url())?;
        launch_browser(&page.0)?;
        // Dropped after its lifetime, or with the runtime when serving ends.
        tokio::spawn(async move {
            tokio::time::sleep(LAUNCHER_LIFETIME).await;
            drop(page);
        });
    }
    server.run().await
}

async fn start(root: &Path, args: ServerArgs, machine: bool) -> Result<Server> {
    let scheduler = Scheduler::open(root, SchedulerConfig::new(EngineMode::Serve)).await?;
    let config = ServeConfig {
        address: SocketAddr::new(args.host, args.port),
        workbench_dir: resolve_workbench(args.workbench_dir),
        allow_remote: args.allow_remote,
    };
    let server = match Server::bind(config, scheduler.clone()).await {
        Ok(server) => server,
        Err(error) => {
            scheduler.shutdown().await;
            return Err(error);
        }
    };
    if args.allow_remote {
        eprintln!("{REMOTE_WARNING}");
    }
    eprintln!(
        "Manim Director {} · project {}",
        env!("CARGO_PKG_VERSION"),
        root.display()
    );
    eprintln!("Workbench: {}", server.url());
    if machine {
        let listening = json!({
            "event": "listening",
            "url": server.url(),
            "address": server.address().to_string(),
        });
        println!("{listening}");
    }
    Ok(server)
}

fn resolve_workbench(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| {
        let development = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workbench/dist");
        development.is_dir().then_some(development)
    })
}

/// A private redirect page for the browser: the token stays out of process
/// arguments, which other local users can read. Removed when dropped.
struct Launcher(PathBuf);

impl Launcher {
    fn write(port: u16, url: &str) -> io::Result<Self> {
        let directory = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|directory| directory.is_dir())
            .unwrap_or_else(env::temp_dir);
        let path = directory.join(format!("manim-director-{port}.html"));
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&path)?;
        write!(
            file,
            "<!doctype html><meta http-equiv=\"refresh\" content=\"0;url={url}\">"
        )?;
        Ok(Self(path))
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn launch_browser(page: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]).arg(page);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(page);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(page);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| anyhow!("could not open a browser: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_launcher_page_is_private_holds_the_url_and_goes_away() {
        use std::os::unix::fs::PermissionsExt;
        let url = "http://127.0.0.1:4177/?token=abc";
        let page = Launcher::write(54_321, url).unwrap();
        let path = page.0.clone();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("content=\"0;url=http://127.0.0.1:4177/?token=abc\""));
        drop(page);
        assert!(!path.exists());
        // A stale page of the same name is replaced.
        fs::write(&path, "stale").unwrap();
        let page = Launcher::write(54_321, url).unwrap();
        assert!(fs::read_to_string(&path)
            .unwrap()
            .starts_with("<!doctype html>"));
        drop(page);
    }
}
