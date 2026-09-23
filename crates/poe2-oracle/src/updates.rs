//! The tray's update entry. It checks GitHub Releases through `auto_update` (on its own after
//! launch when the player allows it, or on a click) and names a newer version when there is one.
//! Clicking that installs it through the silent installer, which relaunches the app.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use auto_update::{UpdateInfo, Version};
use gpui::AsyncApp;
use http_client::HttpClient;
use tray_icon::menu::MenuItem;

use crate::paths;

/// Delay before the automatic check after launch: the price catalogs load first, since they
/// matter more.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(30);

const CHECK_LABEL: &str = "Проверить обновления";

enum State {
    Idle,
    /// A check or an install is running; the entry is disabled.
    Busy,
    Available(UpdateInfo),
}

pub struct Updates {
    item: MenuItem,
    state: RefCell<State>,
    client: Arc<dyn HttpClient>,
    current: Version,
}

impl Updates {
    /// A fresh menu entry to hand to the tray menu and, later, to [`Updates::new`].
    pub fn menu_item() -> MenuItem {
        MenuItem::new(CHECK_LABEL, true, None)
    }

    pub fn new(item: MenuItem, client: Arc<dyn HttpClient>) -> Rc<Self> {
        Rc::new(Updates {
            item,
            state: RefCell::new(State::Idle),
            client,
            current: Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("the crate version is valid semver"),
        })
    }

    /// The automatic check after launch. It reports quietly: nothing new and failures leave the
    /// entry as it is (a failure is only logged, e.g. while no public release exists).
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

    /// The entry was clicked: install the version it names, or check for one.
    pub fn clicked(self: &Rc<Self>, cx: &mut AsyncApp) {
        let available = match &*self.state.borrow() {
            State::Busy => return,
            State::Available(update) => Some(update.clone()),
            State::Idle => None,
        };
        match available {
            Some(update) => self.install(update, cx),
            None => self.check(cx, true),
        }
    }

    fn check(self: &Rc<Self>, cx: &mut AsyncApp, manual: bool) {
        self.set_busy("Проверка обновлений…");
        let this = self.clone();
        cx.spawn(async move |_| {
            let found = auto_update::check_for_update(&this.client, &this.current).await;
            let (label, state) = match found {
                Ok(Some(update)) => (
                    format!("Установить версию {}", update.version),
                    State::Available(update),
                ),
                Ok(None) if manual => ("Установлена последняя версия".to_owned(), State::Idle),
                Ok(None) => (CHECK_LABEL.to_owned(), State::Idle),
                Err(err) => {
                    log::warn!("checking failed: {err:#}");
                    let label = if manual {
                        "Не удалось проверить обновления"
                    } else {
                        CHECK_LABEL
                    };
                    (label.to_owned(), State::Idle)
                }
            };
            this.settle(&label, state);
        })
        .detach();
    }

    /// Downloads and verifies the installer, starts it silently, and quits so it can replace the
    /// exe; the installer relaunches the app. A failed download leaves the offer standing.
    fn install(self: &Rc<Self>, update: UpdateInfo, cx: &mut AsyncApp) {
        self.set_busy(&format!("Загрузка версии {}…", update.version));
        let this = self.clone();
        cx.spawn(async move |cx| {
            let client = this.client.clone();
            let download = update.clone();
            let installer = cx
                .background_executor()
                .spawn(async move {
                    auto_update::download_update(&client, &download, &paths::updates_dir()).await
                })
                .await;
            match installer.and_then(|installer| auto_update::apply_update(&installer)) {
                Ok(()) => cx.update(|cx| cx.quit()),
                Err(err) => {
                    log::warn!("installing {} failed: {err:#}", update.version);
                    let label = format!("Ошибка обновления — повторить ({})", update.version);
                    this.settle(&label, State::Available(update));
                }
            }
        })
        .detach();
    }

    fn set_busy(&self, label: &str) {
        *self.state.borrow_mut() = State::Busy;
        self.item.set_text(label);
        self.item.set_enabled(false);
    }

    fn settle(&self, label: &str, state: State) {
        *self.state.borrow_mut() = state;
        self.item.set_text(label);
        self.item.set_enabled(true);
    }
}
