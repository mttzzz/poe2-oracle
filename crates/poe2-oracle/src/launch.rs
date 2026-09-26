//! What a launch of the app is for, read from its command line: the player's own start, Windows'
//! at sign-in (`platform::autostart`), the installer's finish page just after an install
//! (`packaging/installer.nsi`), or the app's own restart after an update. Only one copy runs per
//! Windows session (`platform::instance`): a launch that finds one running asks it for what the
//! launch was for -- a [`Knock`] on its door -- and exits.
//!
//! Plain argument reading, no Windows API: it builds and is tested on every target.

use std::ffi::OsString;

/// Windows' `Run` value starts the app with this (`platform::autostart`): Windows started it at
/// sign-in, not the player.
pub const AUTOSTART_ARG: &str = "--autostart";
/// The installer's finish page starts the app with this (`packaging/installer.nsi`), when the
/// player leaves «Запустить PoE2 Oracle» ticked: they have just installed it. The silent updates'
/// restarts don't pass it.
pub const INSTALLED_ARG: &str = "--installed";
/// `--after <pid>`: the app restarting itself after an update starts its new copy with this and
/// its own process id, then quits; the new copy waits for that process to end before it claims the
/// session's single copy.
pub const AFTER_ARG: &str = "--after";

/// What the command line says about this launch. Arguments this version doesn't know are
/// ignored: an older copy started by a newer installer or updater still runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Launch {
    /// Windows started it at sign-in ([`AUTOSTART_ARG`]).
    pub autostarted: bool,
    /// The installer's finish page started it ([`INSTALLED_ARG`]): the settings open with the
    /// welcome over them.
    pub installed: bool,
    /// The copy this one replaces after an update, still quitting ([`AFTER_ARG`]): its process id.
    pub after: Option<u32>,
}

/// What a launch that finds a copy running asks it for, knocking on its door
/// (`platform::instance`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knock {
    /// Its settings: what starting the app again most likely means.
    Settings,
    /// Its settings with the welcome over them: the installer has just installed the app.
    Welcome,
}

impl Launch {
    /// The launch `args` describe: the command line after the program's own path.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Launch {
        let mut launch = Launch::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            if arg == AUTOSTART_ARG {
                launch.autostarted = true;
            } else if arg == INSTALLED_ARG {
                launch.installed = true;
            } else if arg == AFTER_ARG {
                // The process id follows. Anything else there -- nothing, another argument -- is no
                // process id: the flag goes unheeded, and that argument is read as its own.
                let pid = args
                    .peek()
                    .and_then(|next| next.to_str())
                    .and_then(|next| next.parse::<u32>().ok())
                    // 0 is Windows' idle process, never an app that could be quitting.
                    .filter(|&pid| pid != 0);
                if pid.is_some() {
                    args.next();
                    launch.after = pid;
                }
            }
        }
        launch
    }

    /// What this launch asks of a copy that is already running, in its place; `None` leaves
    /// quietly. A copy restarting after an update asks nothing: the copy it waited for was
    /// itself, and another one running now wasn't started for the player. Nor does one Windows
    /// started at sign-in. The installer's finish page asks for the welcome, which says the
    /// install worked -- whatever else the command line says.
    pub fn knock(self) -> Option<Knock> {
        if self.after.is_some() {
            None
        } else if self.installed {
            Some(Knock::Welcome)
        } else if self.autostarted {
            None
        } else {
            Some(Knock::Settings)
        }
    }
}

impl Knock {
    /// The number the door's message carries it as. 0 is the settings: the one knock a copy
    /// before the welcome sent, and all it understands of any.
    pub fn word(self) -> usize {
        match self {
            Knock::Settings => 0,
            Knock::Welcome => 1,
        }
    }

    /// The knock a door message's number stands for. One this version doesn't know -- a newer
    /// copy's -- still opens the settings.
    pub fn from_word(word: usize) -> Knock {
        match word {
            1 => Knock::Welcome,
            _ => Knock::Settings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Launch {
        Launch::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn a_launch_without_arguments_is_the_players_own() {
        assert_eq!(parse(&[]), Launch::default());
        assert_eq!(parse(&[]).knock(), Some(Knock::Settings));
    }

    #[test]
    fn only_whole_known_arguments_count() {
        assert!(parse(&["--installed"]).installed);
        assert!(parse(&["--autostart"]).autostarted);
        for near in [
            "--INSTALLED",
            "--installed=1",
            "installed",
            "-installed",
            "--install",
        ] {
            assert!(!parse(&[near]).installed, "{near}");
        }
        assert_eq!(
            parse(&["/S", "--installed", "--from-a-newer-version"]),
            Launch {
                installed: true,
                ..Launch::default()
            },
            "arguments this version doesn't know are passed over"
        );
    }

    #[test]
    fn after_takes_the_process_id_that_follows_it() {
        assert_eq!(parse(&["--after", "4242"]).after, Some(4242));
        assert_eq!(parse(&["--after", "4294967295"]).after, Some(u32::MAX));
    }

    #[test]
    fn after_without_a_usable_process_id_is_unheeded() {
        for value in ["", "0", "-5", "12a", "4294967296", "0x10"] {
            assert_eq!(parse(&["--after", value]).after, None, "{value:?}");
        }
        assert_eq!(parse(&["--after"]).after, None, "nothing follows");
    }

    #[test]
    fn an_argument_where_the_process_id_should_be_is_read_as_its_own() {
        assert_eq!(
            parse(&["--after", "--installed"]),
            Launch {
                installed: true,
                ..Launch::default()
            }
        );
        let launch = parse(&["--after", "77", "--autostart"]);
        assert_eq!((launch.after, launch.autostarted), (Some(77), true));
    }

    #[test]
    fn windows_at_sign_in_leaves_a_running_copy_alone() {
        assert_eq!(parse(&["--autostart"]).knock(), None);
    }

    #[test]
    fn the_installers_launch_asks_for_the_welcome_whatever_else_it_says() {
        assert_eq!(parse(&["--installed"]).knock(), Some(Knock::Welcome));
        assert_eq!(
            parse(&["--autostart", "--installed"]).knock(),
            Some(Knock::Welcome)
        );
    }

    #[test]
    fn a_restart_after_an_update_never_knocks() {
        for args in [
            &["--after", "9"][..],
            &["--after", "9", "--installed"],
            &["--installed", "--after", "9"],
        ] {
            assert_eq!(parse(args).knock(), None, "{args:?}");
        }
        assert_eq!(
            parse(&["--after", "junk"]).knock(),
            Some(Knock::Settings),
            "an unheeded --after is no restart"
        );
    }

    #[test]
    fn a_knock_crosses_the_door_as_it_was_sent() {
        for knock in [Knock::Settings, Knock::Welcome] {
            assert_eq!(Knock::from_word(knock.word()), knock);
        }
        assert_eq!(
            Knock::Settings.word(),
            0,
            "what copies before the welcome send"
        );
        assert_eq!(
            Knock::from_word(7),
            Knock::Settings,
            "a newer copy's knock opens the settings"
        );
    }
}
