//! Whether Windows sees the Internet, so the updater's connection to the app's service
//! (`crate::updates`) comes back as soon as the network does -- not when its next retry is due,
//! up to five minutes after a longer outage. Windows says so through
//! `NotifyNetworkConnectivityHintChange`: the connectivity level its network icon shows, from a
//! thread pool thread. Each move from no Internet to Internet sends on the channel the connection
//! wakes on. The call came with Windows 10 2004, so it's looked up at run time: an exe importing
//! it wouldn't even start on an older Windows, which instead keeps to the connection's own
//! retries.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Context as _, Result, bail};
use windows::Win32::Foundation::{HANDLE, WIN32_ERROR};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{s, w};

/// `NL_NETWORK_CONNECTIVITY_HINT` (nldef.h), as the callback is handed it. Declared here rather
/// than taken from the `windows` crate, whose `Win32_Networking_WinSock` feature would build a
/// whole socket API for one struct. Only the level is read.
#[repr(C)]
#[derive(Clone, Copy)]
struct ConnectivityHint {
    /// An `NL_NETWORK_CONNECTIVITY_LEVEL_HINT`.
    level: i32,
    /// An `NL_NETWORK_CONNECTIVITY_COST_HINT`.
    _cost: i32,
    _approaching_data_limit: u8,
    _over_data_limit: u8,
    _roaming: u8,
}

/// `NetworkConnectivityLevelHintInternetAccess`.
const INTERNET_ACCESS: i32 = 3;
/// `NetworkConnectivityLevelHintConstrainedInternetAccess`: the Internet behind a sign-in page,
/// maybe -- worth a try all the same.
const CONSTRAINED_INTERNET_ACCESS: i32 = 4;

/// `PNETWORK_CONNECTIVITY_HINT_CHANGE_CALLBACK` (netioapi.h).
type HintCallback = unsafe extern "system" fn(context: *const c_void, hint: ConnectivityHint);
/// `NotifyNetworkConnectivityHintChange` (netioapi.h): the initial notification is a `BOOLEAN`.
type NotifyHintChange = unsafe extern "system" fn(
    callback: HintCallback,
    context: *const c_void,
    initial_notification: bool,
    handle: *mut HANDLE,
) -> WIN32_ERROR;
/// `CancelMibChangeNotify2` (netioapi.h).
type CancelNotify = unsafe extern "system" fn(handle: HANDLE) -> WIN32_ERROR;

/// What Windows said last: nothing yet, no Internet, or the Internet.
const UNKNOWN: u8 = 0;
const OFFLINE: u8 = 1;
const ONLINE: u8 = 2;

/// What the callback reads.
struct Watcher {
    back_online: async_channel::Sender<()>,
    /// [`UNKNOWN`], [`OFFLINE`] or [`ONLINE`].
    state: AtomicU8,
}

/// Sends on the channel [`watch`] took until dropped.
pub struct NetworkWatch {
    handle: HANDLE,
    watcher: *mut Watcher,
    cancel: CancelNotify,
}

/// Sends on `back_online` each time Windows goes from no Internet to the Internet -- not for
/// what it says first, which is where things stand already. Fails on a Windows older than 10
/// 2004, which can't say.
pub fn watch(back_online: async_channel::Sender<()>) -> Result<NetworkWatch> {
    // From System32 only: never a DLL of that name planted elsewhere.
    let module = unsafe { LoadLibraryExW(w!("iphlpapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .context("loading iphlpapi.dll")?;
    let notify = unsafe { GetProcAddress(module, s!("NotifyNetworkConnectivityHintChange")) };
    let cancel = unsafe { GetProcAddress(module, s!("CancelMibChangeNotify2")) };
    let (Some(notify), Some(cancel)) = (notify, cancel) else {
        bail!("this Windows can't say when the Internet is back (Windows 10 2004 and later can)");
    };
    // SAFETY: the two functions' signatures, as netioapi.h declares them.
    let notify = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, NotifyHintChange>(notify)
    };
    let cancel = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, CancelNotify>(cancel)
    };
    let watcher = Box::into_raw(Box::new(Watcher {
        back_online,
        state: AtomicU8::new(UNKNOWN),
    }));
    let mut handle = HANDLE::default();
    // SAFETY: `watcher` stays until the notification is cancelled (`Drop`).
    let status = unsafe { notify(on_hint, watcher.cast_const().cast(), true, &mut handle) };
    if status.0 != 0 {
        // SAFETY: never handed out: Windows took no notification.
        drop(unsafe { Box::from_raw(watcher) });
        bail!("NotifyNetworkConnectivityHintChange failed: {status:?}");
    }
    Ok(NetworkWatch {
        handle,
        watcher,
        cancel,
    })
}

impl Drop for NetworkWatch {
    fn drop(&mut self) {
        // Returns once a callback under way has: the watcher can go after it.
        let status = unsafe { (self.cancel)(self.handle) };
        if status.0 == 0 {
            // SAFETY: from `Box::into_raw` in `watch`, and no callback reads it any more.
            drop(unsafe { Box::from_raw(self.watcher) });
        } else {
            // Left as it is: a callback may still come.
            log::warn!("CancelMibChangeNotify2 failed: {status:?}");
        }
    }
}

unsafe extern "system" fn on_hint(context: *const c_void, hint: ConnectivityHint) {
    // SAFETY: the `Watcher` `watch` registered, kept until the notification is cancelled.
    let watcher = unsafe { &*context.cast::<Watcher>() };
    let online = matches!(hint.level, INTERNET_ACCESS | CONSTRAINED_INTERNET_ACCESS);
    let before = watcher
        .state
        .swap(if online { ONLINE } else { OFFLINE }, Ordering::AcqRel);
    if online && before == OFFLINE {
        log::info!("Windows says the Internet is back");
        let _ = watcher.back_online.try_send(());
    }
}
