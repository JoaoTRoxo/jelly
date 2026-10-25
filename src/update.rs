//! Keeping Jelly up to date. Installed with the Windows setup, Jelly updates
//! itself (through Velopack): a newer release on GitHub is downloaded in the
//! background and starts with Jelly the next time, or right away from the
//! toast. On Linux, installed by install.sh, the toast offers to install the
//! new release the same way (see tarball.rs). Anywhere else (the Windows zip,
//! a distribution's package, a build of the source run in place), the toast
//! only says there's one.

use crate::ui::Ctx;
use adw::prelude::*;
use gtk::{gio, glib};
use serde::Deserialize;
use std::rc::Rc;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod tarball;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

enum Found {
    /// Downloaded, and started the next time
    #[cfg(windows)]
    Ready(Box<velopack::UpdateInfo>),
    /// To install over this Jelly, as install.sh did
    #[cfg(target_os = "linux")]
    Installable {
        version: String,
        tarball: tarball::Tarball,
    },
    /// To get by hand: its version and release page
    Available { version: String, page: String },
}

/// Look for a newer release, once, off the main thread
pub fn check(ctx: &Rc<Ctx>) {
    let ctx = Rc::downgrade(ctx);
    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(find).await.ok().flatten();
        let (Some(ctx), Some(found)) = (ctx.upgrade(), found) else {
            return;
        };
        let toast = match found {
            #[cfg(windows)]
            Found::Ready(update) => installed::ready(&ctx, *update),
            #[cfg(target_os = "linux")]
            Found::Installable { version, tarball } => tarball::offer(&ctx, version, tarball),
            Found::Available { version, page } => {
                let toast = adw::Toast::builder()
                    .title(format!("Jelly {version} is available"))
                    .button_label("What's New")
                    .build();
                let window = ctx.window.downgrade();
                toast.connect_button_clicked(move |_| {
                    gtk::UriLauncher::new(&page).launch(
                        window.upgrade().as_ref(),
                        gio::Cancellable::NONE,
                        |_| {},
                    );
                });
                toast
            }
        };
        ctx.toasts.add_toast(toast);
    });
}

fn find() -> Option<Found> {
    #[cfg(windows)]
    if let Some(manager) = installed::manager() {
        return installed::download(&manager).map(|u| Found::Ready(Box::new(u)));
    }
    let release = latest_release()?;
    #[cfg(target_os = "linux")]
    if let Some(tarball) = tarball::Tarball::of(&release) {
        return Some(Found::Installable {
            version: release.version,
            tarball,
        });
    }
    Some(Found::Available {
        version: release.version,
        page: release.page,
    })
}

/// A newer release on GitHub: its version, page and files
struct Release {
    version: String,
    page: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// The latest release on GitHub, if it's newer than this
fn latest_release() -> Option<Release> {
    #[derive(Deserialize)]
    struct Latest {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        assets: Vec<Asset>,
    }
    let url = format!(
        "{}/releases/latest",
        REPOSITORY.replacen("https://github.com/", "https://api.github.com/repos/", 1)
    );
    let response = crate::http::agent(Some(Duration::from_secs(10)))
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()
        .filter(|r| r.status().is_success())?;
    let latest: Latest = crate::http::read_json(response).ok()?;
    is_newer(&latest.tag_name, env!("CARGO_PKG_VERSION")).then(|| Release {
        version: latest.tag_name.trim_start_matches('v').to_string(),
        page: latest.html_url,
        assets: latest.assets,
    })
}

/// Whether `tag` (v0.3.0) is a later version than `current` (0.2.0)
fn is_newer(tag: &str, current: &str) -> bool {
    let numbers = |version: &str| -> Option<Vec<u64>> {
        let version = version.trim_start_matches('v');
        let release = version.split(['-', '+']).next()?;
        release.split('.').map(|n| n.parse().ok()).collect()
    };
    matches!((numbers(tag), numbers(current)), (Some(tag), Some(current)) if tag > current)
}

/// Jelly as the Windows setup installed it, which Velopack updates
#[cfg(windows)]
mod installed {
    use super::*;
    use velopack::sources::{AutoSource, GithubSource};
    use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

    /// Velopack, if the setup installed this Jelly. JELLY_UPDATE_FEED (a
    /// folder or address of releases.win.json) stands in for GitHub, for
    /// testing.
    pub fn manager() -> Option<UpdateManager> {
        match std::env::var("JELLY_UPDATE_FEED") {
            Ok(feed) => UpdateManager::new(AutoSource::new(&feed), None, None),
            Err(_) => UpdateManager::new(GithubSource::new(REPOSITORY, None, false), None, None),
        }
        .ok()
    }

    /// The newer release, downloaded, if there is one
    pub fn download(manager: &UpdateManager) -> Option<UpdateInfo> {
        let update = match manager.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(update)) => *update,
            Ok(_) => return None,
            Err(e) => {
                eprintln!("couldn't check for updates: {e}");
                return None;
            }
        };
        match manager.download_updates(&update, None) {
            Ok(()) => Some(update),
            Err(e) => {
                eprintln!(
                    "couldn't download Jelly {}: {e}",
                    update.TargetFullRelease.Version
                );
                None
            }
        }
    }

    /// Say it starts the next time, or now: Jelly closes as from its close
    /// button (saving where playback stopped), and Velopack starts it again
    /// once it's updated
    pub fn ready(ctx: &Rc<Ctx>, update: UpdateInfo) -> adw::Toast {
        let toast = adw::Toast::builder()
            .title(format!(
                "Jelly {} starts the next time you open it",
                update.TargetFullRelease.Version
            ))
            .button_label("Restart Now")
            .build();
        let c = Rc::downgrade(ctx);
        toast.connect_button_clicked(move |_| {
            let (Some(ctx), Some(manager)) = (c.upgrade(), manager()) else {
                return;
            };
            match manager.wait_exit_then_apply_updates(&update, true, true, Vec::<String>::new()) {
                Ok(()) => ctx.window.close(),
                Err(e) => ctx.toast(&format!("Couldn't update Jelly: {e}")),
            }
        });
        toast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.99.0"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.5", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
        assert!(is_newer("v0.2.1-rc.1", "0.2.0"));
    }
}
