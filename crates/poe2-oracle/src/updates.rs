//! The tray's update entry. It asks the app's web service (oracle.pushka.biz) for the latest
//! release through `auto_update` (on its own after launch when the player allows it, or on a
//! click) and names a newer version when there is one. Clicking that asks for the latest release
//! again and installs it through the silent installer, which relaunches the app.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use auto_update::Version;
use gpui::AsyncApp;
use http_client::HttpClient;
use tray_icon::menu::MenuItem;

use crate::paths;
use crate::tr;

/// Delay before the automatic check after launch: the price catalogs load first, since they
/// matter more.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(30);

enum State {
    Idle,
    /// A check or an install is running; the entry is disabled.
    Busy,
    /// A newer version was found, this one; a click installs the latest release
    /// ([`Updates::install`]).
    Available(Version),
}

/// What the entry says, kept to say it again in another interface language ([`Updates::relabel`]).
enum Label {
    Check,
    Checking,
    UpToDate,
    CheckFailed,
    Install(Version),
    Downloading(Version),
    InstallFailed(Version),
}

impl Label {
    fn text(&self) -> String {
        match self {
            Label::Check => tr!("Check for updates").to_owned(),
            Label::Checking => tr!("Checking for updates…").to_owned(),
            Label::UpToDate => tr!("You have the latest version").to_owned(),
            Label::CheckFailed => tr!("Couldn't check for updates").to_owned(),
            Label::Install(version) => tr!("Install version {version}", version = version),
            Label::Downloading(version) => {
                tr!("Downloading version {version}…", version = version)
            }
            Label::InstallFailed(version) => {
                tr!("Update failed — retry ({version})", version = version)
            }
        }
    }
}

pub struct Updates {
    item: MenuItem,
    state: RefCell<State>,
    label: RefCell<Label>,
    client: Arc<dyn HttpClient>,
    current: Version,
}

impl Updates {
    /// A fresh menu entry to hand to the tray menu and, later, to [`Updates::new`].
    pub fn menu_item() -> MenuItem {
        MenuItem::new(Label::Check.text(), true, None)
    }

    pub fn new(item: MenuItem, client: Arc<dyn HttpClient>) -> Rc<Self> {
        Rc::new(Updates {
            item,
            state: RefCell::new(State::Idle),
            label: RefCell::new(Label::Check),
            client,
            current: Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("the crate version is valid semver"),
        })
    }

    /// Says what the entry says again, in the interface language as it is now.
    pub fn relabel(&self) {
        self.item.set_text(self.label.borrow().text());
    }

    /// The automatic check after launch. It reports quietly: nothing new and failures leave the
    /// entry as it is (a failure is only logged, e.g. while the service knows no release).
    pub fn check_after_launch(self: &Rc<Self>, cx: &mut AsyncApp) {
        let this = self.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(STARTUP_CHECK_DELAY).await;
            if matches!(*this.state.borrow(), State::Idle) {
                this.check(cx, false);
            }
        })
        .detach();
    }

    /// The entry was clicked: install the latest release when it offers one, or check for one.
    pub fn clicked(self: &Rc<Self>, cx: &mut AsyncApp) {
        let offered = match &*self.state.borrow() {
            State::Busy => return,
            State::Available(version) => Some(version.clone()),
            State::Idle => None,
        };
        match offered {
            Some(version) => self.install(version, cx),
            None => self.check(cx, true),
        }
    }

    fn check(self: &Rc<Self>, cx: &mut AsyncApp, manual: bool) {
        self.set_busy(Label::Checking);
        let this = self.clone();
        cx.spawn(async move |_| {
            let found =
                auto_update::check_for_update(&this.client, &this.current, &paths::updates_dir())
                    .await;
            let (label, state) = match found {
                Ok(Some(update)) => (
                    Label::Install(update.version.clone()),
                    State::Available(update.version),
                ),
                Ok(None) if manual => (Label::UpToDate, State::Idle),
                Ok(None) => (Label::Check, State::Idle),
                Err(err) => {
                    log::warn!("checking failed: {err:#}");
                    let label = if manual {
                        Label::CheckFailed
                    } else {
                        Label::Check
                    };
                    (label, State::Idle)
                }
            };
            this.settle(label, state);
        })
        .detach();
    }

    /// Installs the latest release, whichever it is by now: asks the service for it again first --
    /// it hands out the latest release's files only, so an offer from days ago may be gone, and
    /// asking costs an empty `304` while nothing changed. Then downloads its installer and verifies
    /// it against the release's signed SHA256SUMS, starts it silently, and quits so it can replace
    /// the exe; the installer relaunches the app. A failure leaves the offer standing, and the next
    /// click asks again.
    fn install(self: &Rc<Self>, offered: Version, cx: &mut AsyncApp) {
        self.set_busy(Label::Downloading(offered.clone()));
        let this = self.clone();
        cx.spawn(async move |cx| {
            let found =
                auto_update::check_for_update(&this.client, &this.current, &paths::updates_dir())
                    .await;
            let update = match found {
                Ok(Some(update)) => update,
                Ok(None) => {
                    log::info!("{offered} was withdrawn: no release is newer than this version");
                    this.settle(Label::UpToDate, State::Idle);
                    return;
                }
                Err(err) => {
                    log::warn!("checking before installing {offered} failed: {err:#}");
                    this.settle(
                        Label::InstallFailed(offered.clone()),
                        State::Available(offered),
                    );
                    return;
                }
            };
            let version = update.version.clone();
            if version != offered {
                log::info!("the latest release is {version} now, not the offered {offered}");
                this.show(Label::Downloading(version.clone()));
            }
            let client = this.client.clone();
            let installer = cx
                .background_executor()
                .spawn(async move {
                    auto_update::download_update(&client, &update, &paths::updates_dir()).await
                })
                .await;
            match installer.and_then(|installer| auto_update::apply_update(&installer)) {
                Ok(()) => cx.update(crate::app::quit),
                Err(err) => {
                    log::warn!("installing {version} failed: {err:#}");
                    let label = Label::InstallFailed(version.clone());
                    this.settle(label, State::Available(version));
                }
            }
        })
        .detach();
    }

    fn set_busy(&self, label: Label) {
        *self.state.borrow_mut() = State::Busy;
        self.show(label);
        self.item.set_enabled(false);
    }

    fn settle(&self, label: Label, state: State) {
        *self.state.borrow_mut() = state;
        self.show(label);
        self.item.set_enabled(true);
    }

    fn show(&self, label: Label) {
        self.item.set_text(label.text());
        *self.label.borrow_mut() = label;
    }
}
