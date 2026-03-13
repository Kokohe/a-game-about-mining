use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use directories::ProjectDirs;

/// Core game state for the idle game.
///
/// For now we model a single resource (`gold`) and a single generator type
/// (`miners`) that produce gold over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub gold: f64,
    pub miners: u32,
    pub miner_base_rate_per_sec: f64,
    pub last_update: Option<SystemTime>,
}

impl GameState {
    /// Create a fresh game state.
    pub fn new() -> Self {
        Self {
            gold: 0.0,
            miners: 0,
            miner_base_rate_per_sec: 1.0,
            last_update: None,
        }
    }

    /// Advance the simulation to `now`, adding resources based on elapsed time.
    pub fn tick(&mut self, now: SystemTime) {
        if let Some(last) = self.last_update {
            if let Ok(elapsed) = now.duration_since(last) {
                let dt = elapsed.as_secs_f64();
                if dt > 0.0 && self.miners > 0 {
                    let produced = self.miners as f64 * self.miner_base_rate_per_sec * dt;
                    self.gold += produced;
                }
            }
        }
        self.last_update = Some(now);
    }

    /// Current cost of buying one more miner.
    ///
    /// Simple exponential curve so costs rise as you buy more.
    pub fn miner_cost(&self) -> f64 {
        let base_cost = 10.0;
        let growth = 1.15_f64;
        base_cost * growth.powi(self.miners as i32)
    }

    pub fn can_buy_miner(&self) -> bool {
        self.gold >= self.miner_cost()
    }

    pub fn buy_miner(&mut self) -> bool {
        let cost = self.miner_cost();
        if self.gold >= cost {
            self.gold -= cost;
            self.miners += 1;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("could not determine save path")]
    NoPath,
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("serialization error")]
    Serde(#[from] serde_json::Error),
}

fn save_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "IdleGame", "IdleGame")
        .map(|dirs| dirs.data_dir().to_path_buf())
}

fn save_file_path() -> Option<PathBuf> {
    save_dir().map(|mut dir| {
        dir.push("save.json");
        dir
    })
}

pub fn save_game(state: &GameState) -> Result<(), SaveError> {
    let path = save_file_path().ok_or(SaveError::NoPath)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(state)?;
    let mut file = fs::File::create(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn load_game() -> Result<GameState, SaveError> {
    let path = save_file_path().ok_or(SaveError::NoPath)?;
    let mut file = fs::File::open(&path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let mut state: GameState = serde_json::from_str(&buf)?;

    // If last_update is missing or obviously in the future, reset it to now.
    if state.last_update.is_none() {
        state.last_update = Some(SystemTime::now());
    }

    Ok(state)
}

/// Load a saved game if present, otherwise return a fresh state.
pub fn load_or_new() -> GameState {
    match load_game() {
        Ok(mut state) => {
            // Apply offline progress up to now.
            let now = SystemTime::now();
            state.tick(now);
            state
        }
        Err(_) => GameState::new(),
    }
}

