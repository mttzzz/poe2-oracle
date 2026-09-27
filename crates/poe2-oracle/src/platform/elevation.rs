//! Whether the game runs with more rights than this app. Windows doesn't let a program put input
//! into one that runs at a higher integrity level -- the game started "as administrator" while
//! this app wasn't (User Interface Privilege Isolation) -- and tells neither of them: a price
//! check's copy combo and a quick action's keys (`platform::synth_input`) never arrive, and the
//! press does nothing at all. So a press says why instead (`price_check`), and the settings window
//! lists it with the setup's other problems (`diagnostics::setup_problems`). EE2 says the same in
//! a dialog (`main/src/windowing/OverlayWindow.ts`).
//!
//! What the game's token shows is read on Windows ([`game_keys_blocked`]); what it means is
//! decided apart from the Windows calls ([`keys_blocked`]), so it builds and is tested on every
//! target.

/// What this app can tell of the game's rights from its process token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameRights {
    /// The token says the game runs as administrator.
    Elevated,
    /// The token says the game runs with the user's own rights.
    Ordinary,
    /// Windows won't show this app the game's token. It refuses a program without administrator
    /// rights the token of a program that has them, so a refusal is how an elevated game looks
    /// from here, and it counts as one.
    Hidden,
}

/// Whether Windows keeps this app's keys from the game: the game runs as administrator, or looks
/// it ([`GameRights::Hidden`]), and this app doesn't. An app that does reaches any game.
pub fn keys_blocked(game: GameRights, app_elevated: bool) -> bool {
    !app_elevated && game != GameRights::Ordinary
}

#[cfg(target_os = "windows")]
pub use tokens::game_keys_blocked;

#[cfg(target_os = "windows")]
mod tokens {
    use std::sync::LazyLock;

    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    use super::{GameRights, keys_blocked};
    use crate::platform::game_window;

    /// Whether this app runs as administrator: asked once, as that can't change while it runs.
    static APP_ELEVATED: LazyLock<bool> = LazyLock::new(|| {
        // SAFETY: the pseudo-handle of this process, which is never closed.
        token_elevated(unsafe { GetCurrentProcess() }) == Some(true)
    });

    /// Whether the game runs and Windows keeps this app's keys from it ([`keys_blocked`]).
    /// `false` without a game window. A handful of calls: cheap enough for every press of a
    /// hotkey, so a game restarted with other rights is seen at once.
    pub fn game_keys_blocked() -> bool {
        game_rights().is_some_and(|game| keys_blocked(game, *APP_ELEVATED))
    }

    /// The rights of the process behind the game's window; `None` without the window, or once
    /// that process is gone.
    fn game_rights() -> Option<GameRights> {
        let window = game_window::game_window()?;
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid == 0 {
            return None;
        }
        // A refusal of the process itself keeps its token out of reach as well: it counts the same.
        let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(process) => process,
            Err(err) if err.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                return Some(GameRights::Hidden);
            }
            Err(_) => return None,
        };
        let elevated = token_elevated(process);
        let _ = unsafe { CloseHandle(process) };
        Some(match elevated {
            Some(true) => GameRights::Elevated,
            Some(false) => GameRights::Ordinary,
            None => GameRights::Hidden,
        })
    }

    /// Whether `process` runs as administrator, as its token's `TokenElevation` says -- `false`
    /// if the token doesn't say; `None` if Windows won't open the token at all.
    fn token_elevated(process: HANDLE) -> Option<bool> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut written = 0u32;
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&raw mut elevation).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut written,
            )
        };
        let _ = unsafe { CloseHandle(token) };
        Some(read.is_ok() && elevation.TokenIsElevated != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_run_as_administrator_or_hiding_its_token_is_out_of_an_ordinary_apps_reach() {
        assert!(keys_blocked(GameRights::Elevated, false));
        assert!(keys_blocked(GameRights::Hidden, false));
        assert!(!keys_blocked(GameRights::Ordinary, false));
    }

    #[test]
    fn an_app_run_as_administrator_reaches_any_game() {
        for game in [
            GameRights::Elevated,
            GameRights::Hidden,
            GameRights::Ordinary,
        ] {
            assert!(!keys_blocked(game, true), "{game:?}");
        }
    }
}
