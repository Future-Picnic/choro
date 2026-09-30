use std::process::Command;
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity};

const PLAYBACK_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpotifyPlaybackState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

pub struct CompanionMusicState {
    engaged: bool,
    active_playlist: Option<usize>,
    playback: SpotifyPlaybackState,
    error: Option<String>,
    generation: u64,
    command_in_flight: bool,
}

impl CompanionMusicState {
    pub fn view(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.spawn(async move |this, cx| loop {
                let Some(state) = this.upgrade() else {
                    break;
                };
                let poll_generation = state
                    .update(cx, |state: &mut Self, _| state.poll_generation())
                    .ok()
                    .flatten();
                if let Some(poll_generation) = poll_generation {
                    let playback = cx
                        .background_executor()
                        .spawn(async { playback_state() })
                        .await;
                    if state
                        .update(cx, |state: &mut Self, cx| {
                            if state.apply_poll_result(poll_generation, playback) {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                cx.background_executor().timer(PLAYBACK_POLL_INTERVAL).await;
            })
            .detach();

            Self {
                engaged: false,
                active_playlist: None,
                playback: SpotifyPlaybackState::Stopped,
                error: None,
                generation: 0,
                command_in_flight: false,
            }
        })
    }

    pub fn play(&mut self, index: usize, playlist_uri: String, cx: &mut Context<Self>) {
        self.engaged = true;
        self.active_playlist = Some(index);
        self.playback = SpotifyPlaybackState::Stopped;
        self.error = None;
        let generation = self.begin_command();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { play_playlist(&playlist_uri) })
                .await;
            let playback = if result.is_ok() {
                cx.background_executor()
                    .spawn(async { playback_state() })
                    .await
            } else {
                Ok(SpotifyPlaybackState::Stopped)
            };
            this.update(cx, |state, cx| {
                if state.apply_play_result(generation, result, playback) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        if !self.is_playing() {
            return;
        }
        self.playback = SpotifyPlaybackState::Paused;
        self.error = None;
        let generation = self.begin_command();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { pause_playback() })
                .await;
            this.update(cx, |state, cx| {
                if state.apply_pause_result(generation, result) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if !self.engaged || self.active_playlist.is_none() || self.is_playing() {
            return;
        }
        self.playback = SpotifyPlaybackState::Playing;
        self.error = None;
        let generation = self.begin_command();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { resume_playback() })
                .await;
            this.update(cx, |state, cx| {
                if state.apply_resume_result(generation, result) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn is_playing(&self) -> bool {
        self.engaged && self.playback == SpotifyPlaybackState::Playing
    }

    pub fn active_playlist(&self) -> Option<usize> {
        self.active_playlist
    }

    pub fn is_paused(&self) -> bool {
        self.engaged && self.playback == SpotifyPlaybackState::Paused
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn begin_command(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.command_in_flight = true;
        self.generation
    }

    fn poll_generation(&self) -> Option<u64> {
        (self.engaged && !self.command_in_flight).then_some(self.generation)
    }

    fn apply_poll_result(
        &mut self,
        generation: u64,
        result: Result<SpotifyPlaybackState, String>,
    ) -> bool {
        if self.command_in_flight || self.generation != generation {
            return false;
        }
        match result {
            Ok(playback) => {
                self.playback = playback;
                self.error = None;
            }
            Err(error) => {
                self.playback = SpotifyPlaybackState::Stopped;
                self.error = Some(error);
            }
        }
        true
    }

    fn apply_play_result(
        &mut self,
        generation: u64,
        result: Result<(), String>,
        playback: Result<SpotifyPlaybackState, String>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        self.command_in_flight = false;
        match result {
            Ok(()) => match playback {
                Ok(playback) => {
                    self.playback = playback;
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            },
            Err(error) => {
                self.engaged = false;
                self.active_playlist = None;
                self.playback = SpotifyPlaybackState::Stopped;
                self.error = Some(error);
            }
        }
        true
    }

    fn apply_pause_result(&mut self, generation: u64, result: Result<(), String>) -> bool {
        if self.generation != generation {
            return false;
        }
        self.command_in_flight = false;
        if let Err(error) = result {
            self.playback = SpotifyPlaybackState::Playing;
            self.error = Some(error);
        }
        true
    }

    fn apply_resume_result(&mut self, generation: u64, result: Result<(), String>) -> bool {
        if self.generation != generation {
            return false;
        }
        self.command_in_flight = false;
        if let Err(error) = result {
            self.playback = SpotifyPlaybackState::Paused;
            self.error = Some(error);
        }
        true
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    fn state(playback: SpotifyPlaybackState) -> CompanionMusicState {
        CompanionMusicState {
            engaged: true,
            active_playlist: Some(0),
            playback,
            error: None,
            generation: 0,
            command_in_flight: false,
        }
    }

    #[test]
    fn poll_started_before_pause_cannot_restore_playing() {
        let mut state = state(SpotifyPlaybackState::Playing);
        let poll_generation = state.poll_generation().unwrap();
        state.playback = SpotifyPlaybackState::Paused;
        let pause_generation = state.begin_command();

        assert!(!state.apply_poll_result(poll_generation, Ok(SpotifyPlaybackState::Playing)));
        assert_eq!(state.playback, SpotifyPlaybackState::Paused);
        assert!(state.apply_pause_result(pause_generation, Ok(())));
        assert_eq!(state.poll_generation(), Some(pause_generation));
    }

    #[test]
    fn stale_play_completion_cannot_overwrite_newer_playlist() {
        let mut state = state(SpotifyPlaybackState::Stopped);
        let first_generation = state.begin_command();
        state.active_playlist = Some(1);
        let second_generation = state.begin_command();

        assert!(!state.apply_play_result(
            first_generation,
            Err("old request failed".to_string()),
            Ok(SpotifyPlaybackState::Stopped),
        ));
        assert_eq!(state.active_playlist, Some(1));
        assert!(state.command_in_flight);

        assert!(state.apply_play_result(
            second_generation,
            Ok(()),
            Ok(SpotifyPlaybackState::Playing),
        ));
        assert_eq!(state.active_playlist, Some(1));
        assert_eq!(state.playback, SpotifyPlaybackState::Playing);
        assert!(!state.command_in_flight);
    }

    #[test]
    fn polling_waits_until_the_current_command_finishes() {
        let mut state = state(SpotifyPlaybackState::Paused);
        let generation = state.begin_command();
        assert_eq!(state.poll_generation(), None);

        assert!(state.apply_resume_result(generation, Ok(())));
        assert_eq!(state.poll_generation(), Some(generation));
    }
}

pub fn play_playlist(playlist_uri: &str) -> Result<(), String> {
    if ide_core::config::spotify_playlist_uri(playlist_uri).as_deref() != Some(playlist_uri) {
        return Err("That is not a valid Spotify playlist.".to_string());
    }
    platform::play_playlist(playlist_uri)
}

pub fn playback_state() -> Result<SpotifyPlaybackState, String> {
    platform::playback_state()
}

pub fn pause_playback() -> Result<(), String> {
    platform::pause_playback()
}

pub fn resume_playback() -> Result<(), String> {
    platform::resume_playback()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const PLAY_PLAYLIST_SCRIPT: &str = r#"
on run argv
    set playlistUri to item 1 of argv
    tell application "Spotify"
        launch
        play track playlistUri
        -- Spotify begins a playlist at its first track even when shuffle was
        -- previously enabled. Once the context is loaded, enable shuffle and
        -- advance once so each companion session starts somewhere different
        -- and continues in randomized order. Playback remains usable on
        -- accounts where Spotify declines either optional command.
        delay 0.4
        try
            set shuffling to true
            next track
        end try
    end tell
end run
"#;

    const PLAYER_STATE_SCRIPT: &str = r#"
tell application "Spotify"
    return player state as string
end tell
"#;

    const PAUSE_SCRIPT: &str = r#"
tell application "Spotify"
    pause
end tell
"#;

    const RESUME_SCRIPT: &str = r#"
tell application "Spotify"
    launch
    play
end tell
"#;

    pub fn play_playlist(playlist_uri: &str) -> Result<(), String> {
        if !std::path::Path::new("/Applications/Spotify.app").is_dir() {
            return Err("Install the Spotify desktop app in Applications first.".to_string());
        }
        run_osascript(PLAY_PLAYLIST_SCRIPT, &[playlist_uri]).map(|_| ())
    }

    pub fn playback_state() -> Result<SpotifyPlaybackState, String> {
        if !spotify_is_running() {
            return Ok(SpotifyPlaybackState::Stopped);
        }
        let output = run_osascript(PLAYER_STATE_SCRIPT, &[])?;
        Ok(parse_playback_state(&output))
    }

    pub fn pause_playback() -> Result<(), String> {
        if !spotify_is_running() {
            return Ok(());
        }
        run_osascript(PAUSE_SCRIPT, &[]).map(|_| ())
    }

    pub fn resume_playback() -> Result<(), String> {
        if !std::path::Path::new("/Applications/Spotify.app").is_dir() {
            return Err("Install the Spotify desktop app in Applications first.".to_string());
        }
        run_osascript(RESUME_SCRIPT, &[]).map(|_| ())
    }

    fn spotify_is_running() -> bool {
        Command::new("pgrep")
            .args(["-x", "Spotify"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn run_osascript(script: &str, arguments: &[&str]) -> Result<String, String> {
        let mut command = Command::new("osascript");
        command.args(["-e", script]);
        if !arguments.is_empty() {
            command.arg("--").args(arguments);
        }
        let output = command
            .output()
            .map_err(|error| format!("Could not control Spotify: {error}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
        let detail =
            ide_core::redact_sensitive_text(String::from_utf8_lossy(&output.stderr).trim());
        if detail.is_empty() {
            Err("Spotify did not accept the playback command.".to_string())
        } else {
            Err(format!(
                "Spotify did not accept the playback command: {detail}"
            ))
        }
    }

    fn parse_playback_state(value: &str) -> SpotifyPlaybackState {
        match value.trim().to_ascii_lowercase().as_str() {
            "playing" => SpotifyPlaybackState::Playing,
            "paused" => SpotifyPlaybackState::Paused,
            _ => SpotifyPlaybackState::Stopped,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_spotify_desktop_player_states() {
            assert_eq!(
                parse_playback_state("playing\n"),
                SpotifyPlaybackState::Playing
            );
            assert_eq!(parse_playback_state("paused"), SpotifyPlaybackState::Paused);
            assert_eq!(
                parse_playback_state("stopped"),
                SpotifyPlaybackState::Stopped
            );
            assert_eq!(
                parse_playback_state("unknown"),
                SpotifyPlaybackState::Stopped
            );
        }

        #[test]
        fn playlist_playback_enables_shuffle_and_skips_the_fixed_first_track() {
            let shuffle = PLAY_PLAYLIST_SCRIPT
                .find("set shuffling to true")
                .expect("playback should enable Spotify shuffle");
            let advance = PLAY_PLAYLIST_SCRIPT
                .find("next track")
                .expect("playback should advance from the fixed first track");
            assert!(shuffle < advance);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn play_playlist(_playlist_uri: &str) -> Result<(), String> {
        Err("Companion Spotify playback is currently available on macOS only.".to_string())
    }

    pub fn playback_state() -> Result<SpotifyPlaybackState, String> {
        Ok(SpotifyPlaybackState::Stopped)
    }

    pub fn pause_playback() -> Result<(), String> {
        Err("Companion Spotify playback is currently available on macOS only.".to_string())
    }

    pub fn resume_playback() -> Result<(), String> {
        Err("Companion Spotify playback is currently available on macOS only.".to_string())
    }
}
