//! Updating a Jelly that install.sh put in place (from a release's tarball,
//! or built from the source): the new release's tarball, checked against the
//! release's SHA256SUMS, installed by its own install.sh into the same place.

use super::Release;
use crate::ui::Ctx;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

/// The most the tarball (2 MB in 0.3.0) or SHA256SUMS is read of
const LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct Tarball {
    /// jelly-0.4.0-linux-x86_64, the folder in it too
    name: String,
    url: String,
    sums_url: String,
    /// Where install.sh put this Jelly (its PREFIX)
    prefix: PathBuf,
}

impl Tarball {
    /// The release's tarball for this machine, if this Jelly can be updated
    /// with it
    pub fn of(release: &Release) -> Option<Tarball> {
        tarball_for(release, std::env::consts::ARCH, prefix()?)
    }
}

fn tarball_for(release: &Release, arch: &str, prefix: PathBuf) -> Option<Tarball> {
    let name = format!("jelly-{}-linux-{arch}", release.version);
    let url = |file: &str| {
        let asset = release.assets.iter().find(|a| a.name == file)?;
        Some(asset.browser_download_url.clone())
    };
    Some(Tarball {
        url: url(&format!("{name}.tar.gz"))?,
        sums_url: url("SHA256SUMS")?,
        name,
        prefix,
    })
}

/// Where install.sh put this Jelly, PREFIX/bin/jelly next to its launcher
/// entry, if it did and it's ours to replace (not a distribution's)
fn prefix() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?;
    let bin = program.parent()?;
    let prefix = bin.parent()?;
    let entry = prefix.join(format!("share/applications/{}.desktop", crate::APP_ID));
    let installed = program.file_name()? == "jelly" && bin.file_name()? == "bin" && entry.is_file();
    (installed && writable(bin)).then(|| prefix.to_path_buf())
}

fn writable(folder: &Path) -> bool {
    let can_write = "access::can-write";
    gio::File::for_path(folder)
        .query_info(
            can_write,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .is_ok_and(|info| info.boolean(can_write))
}

/// "Jelly X is available", with Update
pub fn offer(ctx: &Rc<Ctx>, version: String, tarball: Tarball) -> adw::Toast {
    let toast = adw::Toast::builder()
        .title(format!("Jelly {version} is available"))
        .button_label("Update")
        .build();
    let c = Rc::downgrade(ctx);
    toast.connect_button_clicked(move |_| {
        let Some(ctx) = c.upgrade() else { return };
        ctx.toast(&format!("Updating Jelly to {version}…"));
        let (c, version, tarball) = (Rc::downgrade(&ctx), version.clone(), tarball.clone());
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || install(&tarball)).await;
            let Some(ctx) = c.upgrade() else { return };
            match done {
                Ok(Ok(program)) => ctx.toasts.add_toast(installed(&ctx, &version, program)),
                Ok(Err(e)) => ctx.toast(&format!("Couldn't update Jelly: {e}")),
                Err(_) => ctx.toast("Couldn't update Jelly"),
            }
        });
    });
    toast
}

/// "Jelly X is installed", with Restart Now: Jelly closes as from its close
/// button (saving where playback stopped), and starts again once it's gone.
/// Started before, the new one would only hand itself to this one, over
/// D-Bus.
fn installed(ctx: &Rc<Ctx>, version: &str, program: PathBuf) -> adw::Toast {
    let toast = adw::Toast::builder()
        .title(format!("Jelly {version} is installed"))
        .button_label("Restart Now")
        .build();
    let c = Rc::downgrade(ctx);
    toast.connect_button_clicked(move |_| {
        let Some(ctx) = c.upgrade() else { return };
        let wait_then_start = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.1; done; exec "$2""#;
        let started = gio::Subprocess::newv(
            &[
                "sh".as_ref(),
                "-c".as_ref(),
                wait_then_start.as_ref(),
                "sh".as_ref(),
                std::process::id().to_string().as_ref(),
                program.as_os_str(),
            ],
            gio::SubprocessFlags::NONE,
        );
        match started {
            Ok(_) => ctx.window.close(),
            Err(e) => ctx.toast(&format!("Couldn't start Jelly again: {e}")),
        }
    });
    toast
}

/// Download the tarball, check it, and have its install.sh install it where
/// this Jelly is: the program, as installed
fn install(tarball: &Tarball) -> Result<PathBuf, String> {
    let file = format!("{}.tar.gz", tarball.name);
    let sums = download(&tarball.sums_url)?;
    let sums = String::from_utf8_lossy(&sums);
    let expected =
        checksum_of(&sums, &file).ok_or_else(|| format!("{file} isn't in SHA256SUMS"))?;
    let data = download(&tarball.url)?;
    let actual = glib::compute_checksum_for_data(glib::ChecksumType::Sha256, &data)
        .ok_or("couldn't work out its checksum")?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("{file} doesn't match its checksum"));
    }
    let work = glib::user_cache_dir().join("jelly").join("update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let archive = work.join(&file);
    std::fs::write(&archive, &data).map_err(|e| e.to_string())?;
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&work))?;
    run(Command::new("sh")
        .arg(work.join(&tarball.name).join("install.sh"))
        .env("PREFIX", &tarball.prefix))?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(tarball.prefix.join("bin").join("jelly"))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = crate::http::agent(Some(Duration::from_secs(120)))
        .get(url)
        .call()
        .map_err(|e| crate::http::problem(&e))?;
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}", response.status()));
    }
    response
        .body_mut()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()
        .map_err(|e| crate::http::problem(&e))
}

/// Run `command`, and on failure say why (the last line it printed)
fn run(command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let printed = String::from_utf8_lossy(&output.stderr);
    Err(printed
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map_or_else(|| output.status.to_string(), |l| l.trim().to_string()))
}

/// The SHA-256 that SHA256SUMS ("<sum>  <file>" lines) gives `file`
fn checksum_of<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (sum, name) = line.split_once(char::is_whitespace)?;
        (name.trim_start().trim_start_matches('*') == file).then_some(sum)
    })
}

#[cfg(test)]
mod tests {
    use super::super::Asset;
    use super::*;

    #[test]
    fn finds_the_checksum() {
        let sums = "5fb7  RELEASES\nabc123  jelly-0.4.0-linux-x86_64.tar.gz\n";
        assert_eq!(
            checksum_of(sums, "jelly-0.4.0-linux-x86_64.tar.gz"),
            Some("abc123")
        );
        assert_eq!(
            checksum_of("abc *jelly.tar.gz", "jelly.tar.gz"),
            Some("abc")
        );
        assert_eq!(checksum_of(sums, "jelly-0.4.0-linux-aarch64.tar.gz"), None);
    }

    #[test]
    fn picks_this_machines_tarball() {
        let asset = |name: &str| Asset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
        };
        let release = Release {
            version: "0.4.0".into(),
            page: String::new(),
            assets: vec![
                asset("jelly-0.4.0-linux-x86_64.tar.gz"),
                asset("jelly-0.4.0-windows-x86_64.zip"),
                asset("SHA256SUMS"),
            ],
        };
        let tarball = tarball_for(&release, "x86_64", "/home/a/.local".into()).unwrap();
        assert_eq!(tarball.name, "jelly-0.4.0-linux-x86_64");
        assert_eq!(
            tarball.url,
            "https://example.com/jelly-0.4.0-linux-x86_64.tar.gz"
        );
        assert_eq!(tarball.sums_url, "https://example.com/SHA256SUMS");
        // None for this machine
        assert!(tarball_for(&release, "aarch64", "/home/a/.local".into()).is_none());
    }
}
