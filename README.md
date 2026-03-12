# A Game About Mining (ASCII Idle Prototype)

A small cross-platform idle/automation prototype written in Rust, using `egui` / `eframe` for a native desktop window and ASCII graphics.

The current version focuses on:
- An ASCII **map of `#` tiles** (ground) with a gap along the bottom.
- A single **NPC (`@`)** that moves randomly.
- **Harvesting**: when the NPC walks over a `#`, it removes that ground tile and increases a counter.
- A **counter line** under the map showing how many `#` tiles have been harvested.
- A **circular lane** behavior on the bottom row (horizontal wrap).

## Project structure

Workspace root: `idle-game/`

- `Cargo.toml` – Cargo workspace definition with two members:
  - `game-core`
  - `desktop-app`

### Crate: `game-core`

- `game-core/Cargo.toml`
  - Depends on: `serde`, `serde_json`, `directories`, `thiserror`.

- `game-core/src/lib.rs`
  - **Game state**
    - `GameState` with:
      - `gold`
      - `miners`
      - `miner_base_rate_per_sec`
      - `last_update` for time-based progression.
  - **Logic**
    - `tick(now)` – advances gold based on elapsed real time and number of miners.
    - `miner_cost`, `can_buy_miner`, `buy_miner` – simple exponential cost curve and purchase helpers.
  - **Persistence**
    - `save_game` / `load_game` – JSON save/load in a platform-appropriate data directory.
    - `load_or_new` – loads existing state and applies offline progress, or creates a fresh state.

### Crate: `desktop-app`

- `desktop-app/Cargo.toml`
  - Depends on: `eframe`, `egui`, `rand`, `game-core`.

- `desktop-app/src/main.rs`
  - **World representation**
    - `Tile` enum: `Ground` (`#`) or `Empty` (` `).
    - `World` struct: `width`, `height`, `tiles: Vec<Tile>`.
    - Initializes as all `Ground` with a carved-out gap on the bottom row for walking.
    - `to_ascii(npc_x, npc_y, inventory)` renders the map plus the NPC, then appends `#: {inventory}` under the map.
  - **App state**
    - `App` struct holds:
      - `world: World`
      - `game: GameState` (from `game-core`)
      - `last_tick_instant`, `last_save_instant`
      - `npc_x`, `npc_y`
      - `harvested: u32` – number of mined `#` tiles.
  - **Behavior**
    - Every ~400 ms:
      - Calls `game.tick(now)` for idle resource generation.
      - Calls `step_npc_random()`:
        - Picks a random direction (up, down, left, right).
        - Wraps horizontally on the bottom row to make it circular.
        - If stepping onto `Tile::Ground`, converts it to `Tile::Empty` and increments `harvested`.
    - Autosaves `GameState` every few seconds.
  - **UI**
    - Implements `eframe::App::update`:
      - Builds the ASCII string via `world.to_ascii(...)`.
      - Renders it as monospace text in an `egui::CentralPanel`.
      - Continuously requests repaint so the NPC keeps moving.

## Running

From the workspace root:

```bash
cargo run -p desktop-app
```

This opens a window showing:
- The ASCII mine of `#` tiles.
- An `@` character wandering and mining tiles.
- A `#: {number}` line under the map with the total harvested tiles.

