//! Local-only prompt state and the opt-in product-updates request.
//!
//! No address is stored in Choro's local state. The only network request is
//! made after the user explicitly presses Send me updates.

use std::{fs, path::Path, time::Duration};

use ide_core::AppConfig;
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "product-updates-prompt.json";
const SIGNUP_URL: &str = "https://relay.choro.dev/newsletter/subscribe";

#[derive(Default, Deserialize, Serialize)]
struct PromptState {
    #[serde(default)]
    launches: u8,
    #[serde(default)]
    dismissed: bool,
    #[serde(default)]
    subscribed: bool,
}

impl PromptState {
    fn should_offer(&self) -> bool {
        self.launches >= 2 && !self.dismissed && !self.subscribed
    }
}

fn eligible_build() -> bool {
    std::env::var_os("CHORO_DEMO").is_none() && std::env::var_os("CHORO_ONBOARDING").is_none()
}

fn load_at(root: &Path) -> PromptState {
    fs::read(root.join(STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_at(root: &Path, state: &PromptState) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    let path = root.join(STATE_FILE);
    let temp = root.join(format!("{STATE_FILE}.tmp"));
    let json = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    fs::write(&temp, json)?;
    fs::rename(temp, path)
}

/// Count real launches, including the first launch that shows onboarding.
pub fn record_launch() {
    if !eligible_build() {
        return;
    }
    let root = AppConfig::config_root();
    let mut state = load_at(&root);
    state.launches = state.launches.saturating_add(1).min(2);
    if let Err(error) = save_at(&root, &state) {
        eprintln!("could not save product-updates prompt state: {error}");
    }
}

pub fn should_offer() -> bool {
    if !eligible_build() {
        return false;
    }
    let state = load_at(&AppConfig::config_root());
    state.should_offer()
}

pub fn is_subscribed() -> bool {
    load_at(&AppConfig::config_root()).subscribed
}

pub fn dismiss() {
    update_state(|state| state.dismissed = true);
}

pub fn mark_subscribed() {
    update_state(|state| state.subscribed = true);
}

fn update_state(update: impl FnOnce(&mut PromptState)) {
    let root = AppConfig::config_root();
    let mut state = load_at(&root);
    update(&mut state);
    if let Err(error) = save_at(&root, &state) {
        eprintln!("could not save product-updates prompt state: {error}");
    }
}

pub fn subscribe(email: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Signup is temporarily unavailable.".to_string())?;
    runtime.block_on(subscribe_async(email))
}

async fn subscribe_async(email: &str) -> Result<(), String> {
    let email = email.trim();
    if email.len() > 254 || !email.contains('@') || email.chars().any(char::is_whitespace) {
        return Err("Enter a valid email address.".into());
    }
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "Signup is temporarily unavailable.".to_string())?
        .post(SIGNUP_URL)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::json!({ "email": email, "consent": true }).to_string())
        .send()
        .await
        .map_err(|_| "Could not connect. Please try again.".to_string())?;
    if response.status().is_success() {
        Ok(())
    } else if response.status() == reqwest::StatusCode::CONFLICT {
        Err("This address could not be subscribed. Contact hello@choro.dev for help.".into())
    } else {
        Err("Could not subscribe right now. Please try again.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_starts_after_second_launch_and_stays_dismissed() {
        let root = std::env::temp_dir().join(format!("choro-newsletter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut state = load_at(&root);
        assert_eq!(state.launches, 0);
        assert!(!state.should_offer());
        state.launches = 1;
        save_at(&root, &state).unwrap();
        assert_eq!(load_at(&root).launches, 1);
        assert!(!load_at(&root).should_offer());
        state.launches = 2;
        save_at(&root, &state).unwrap();
        assert_eq!(load_at(&root).launches, 2);
        assert!(load_at(&root).should_offer());
        state.dismissed = true;
        save_at(&root, &state).unwrap();
        assert!(load_at(&root).dismissed);
        assert!(!load_at(&root).should_offer());
        state.dismissed = false;
        state.subscribed = true;
        save_at(&root, &state).unwrap();
        assert!(!load_at(&root).should_offer());
        let _ = fs::remove_dir_all(&root);
    }
}
