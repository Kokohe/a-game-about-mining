use eframe::egui;
use game_core::{self, GameState};
use rand::distributions::{Distribution, WeightedIndex};
use rand::seq::SliceRandom;
use rand::Rng;
use rand::distributions::Uniform;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy)]
enum OreKind {
    Zero,
    Q,
    D,
    G,
    C,
}

#[derive(Clone, Copy)]
enum Tile {
    Ground,
    Empty,
    Ore(OreKind),
    Path,
}

#[derive(Clone)]
struct World {
    width: usize,
    height: usize,
    tiles: Vec<Cell>,
    spawn_x: usize,
    spawn_y: usize,
}

#[derive(Clone)]
struct Cell {
    tile: Tile,
    max_durability: u8,
    durability: u8,
}

impl World {
    fn new(width: usize, height: usize) -> Self {
        // Fill the world with ground and sprinkle multiple ore types using
        // clustered noise so ore tends to appear in clumps.
        use rand::Rng;

        let mut rng = rand::thread_rng();
        let mut tiles = Vec::with_capacity(width * height);

        let bottom_y = height.saturating_sub(1);
        let gap_start = width / 4;
        let gap_end = width / 4 * 3;

        // Center and radius for durability layers: harder the further from center.
        let center_x = width as isize / 2;
        let center_y = height as isize / 2;
        let max_radius = ((center_x.pow(2) + center_y.pow(2)) as f32).sqrt().max(1.0);

        // Ore kind for a given durability layer (1=softest at center, 20=hardest at edges).
        // Layer 1–4: Zero, 5–8: Q, 9–12: D, 13–16: G, 17–20: C.
        let durability_to_ore = |d: u8| -> OreKind {
            match d {
                1..=4 => OreKind::Zero,
                5..=8 => OreKind::Q,
                9..=12 => OreKind::D,
                13..=16 => OreKind::G,
                _ => OreKind::C, // 17..=20
            }
        };

        // Coarse grid: which cells have an ore cluster (ore type is decided by layer later).
        let cell_size = 4usize;
        let coarse_w = (width + cell_size - 1) / cell_size;
        let coarse_h = (height + cell_size - 1) / cell_size;
        let mut coarse_has_ore: Vec<bool> = Vec::with_capacity(coarse_w * coarse_h);
        for _ in 0..(coarse_w * coarse_h) {
            coarse_has_ore.push(rng.gen::<f32>() < 0.25);
        }

        for y in 0..height {
            for x in 0..width {
                if y == bottom_y && (gap_start..gap_end).contains(&x) {
                    tiles.push(Cell {
                        tile: Tile::Empty,
                        max_durability: 0,
                        durability: 0,
                    });
                    continue;
                }

                // Durability from distance only: 1 at center, 20 at edges (no noise for clear layers).
                let dx = x as isize - center_x;
                let dy = y as isize - center_y;
                let r = ((dx * dx + dy * dy) as f32).sqrt();
                let r_norm = (r / max_radius).clamp(0.0, 1.0);
                let d = (1.0 + r_norm * 19.0).round().clamp(1.0, 20.0) as u8;
                let max_durability = d;
                let durability = d;

                // Ore spawns only in its matching hardness layer.
                let kind = durability_to_ore(d);
                let cx = x / cell_size;
                let cy = y / cell_size;
                let coarse_idx = cy * coarse_w + cx;
                let in_ore_cluster = coarse_has_ore[coarse_idx];

                let tile = if in_ore_cluster {
                    if rng.gen::<f32>() < 0.6 {
                        Tile::Ore(kind)
                    } else {
                        Tile::Ground
                    }
                } else {
                    if rng.gen::<f32>() < 0.03 {
                        Tile::Ore(kind)
                    } else {
                        Tile::Ground
                    }
                };

                tiles.push(Cell {
                    tile,
                    max_durability,
                    durability,
                });
            }
        }

        let spawn_x = width / 2;
        let spawn_y = height / 2;

        // Ensure tiles around the time crystal start as empty space so miners can spawn there.
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 {
                    continue; // leave the crystal's own tile as-is
                }
                let nx = spawn_x as isize + dx;
                let ny = spawn_y as isize + dy;
                if nx >= 0 && nx < width as isize && ny >= 0 && ny < height as isize {
                    let idx = (ny as usize) * width + (nx as usize);
                    tiles[idx] = Cell {
                        tile: Tile::Empty,
                        max_durability: 0,
                        durability: 0,
                    };
                }
            }
        }

        Self {
            width,
            height,
            tiles,
            spawn_x,
            spawn_y,
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    fn bottom_y(&self) -> usize {
        self.height.saturating_sub(1)
    }

    /// Convert a viewport of the world + miners + resource counters into ASCII.
    fn to_ascii(
        &self,
        miners: &[Miner],
        grindstone_pos: Option<(usize, usize)>,
        shop_pos: Option<(usize, usize)>,
        town_hall_pos: Option<(usize, usize)>,
        ground_count: u32,
        zero_count: u32,
        q_count: u32,
        d_count: u32,
        g_count: u32,
        c_count: u32,
        steps: u64,
        max_steps: u64,
        view_origin_x: usize,
        view_origin_y: usize,
        view_width: usize,
        view_height: usize,
    ) -> String {
        let mut s = String::with_capacity(view_width * (view_height + 4));

        let x_start = view_origin_x.min(self.width);
        let y_start = view_origin_y.min(self.height);
        let x_end = (x_start + view_width).min(self.width);
        let y_end = (y_start + view_height).min(self.height);

        for y in y_start..y_end {
            for x in x_start..x_end {
                let miner_at_pos =
                    miners.iter().find(|m| m.x == x && m.y == y);
                let home_marker_for_pos =
                    miners.iter().find(|m| m.home_x == x && m.home_y == y);
                let is_spawn_here = x == self.spawn_x && y == self.spawn_y;
                let is_grindstone_here = grindstone_pos == Some((x, y));
                let is_shop_here = shop_pos == Some((x, y));
                let is_town_hall_here = town_hall_pos == Some((x, y));
                let ch = if miner_at_pos.is_some() {
                    '@'
                } else if is_grindstone_here {
                    '*'
                } else if is_shop_here {
                    'S'
                } else if is_town_hall_here {
                    'H'
                } else if is_spawn_here {
                    'T'
                } else if home_marker_for_pos.is_some() {
                    'x'
                } else {
                    match self.tiles[self.idx(x, y)].tile {
                        Tile::Ground => '#',
                        Tile::Empty => ' ',
                        Tile::Path => '.',
                        Tile::Ore(OreKind::Zero) => '0',
                        Tile::Ore(OreKind::Q) => 'Q',
                        Tile::Ore(OreKind::D) => 'D',
                        Tile::Ore(OreKind::G) => 'G',
                        Tile::Ore(OreKind::C) => 'C',
                    }
                };
                s.push(ch);
            }
            s.push('\n');
        }

        // Timer and resource counters under the map.
        use std::fmt::Write as _;
        let _ = write!(&mut s, "Year {}\n", steps);
        let _ = write!(
            &mut s,
            "#: {}  0: {}  Q: {}  D: {}  G: {}  C: {}\n",
            ground_count, zero_count, q_count, d_count, g_count, c_count
        );

        s
    }
}

/// Items a miner can carry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InventoryItem {
    Grindstone,
    Shop,
    TownHall,
}

impl std::fmt::Display for InventoryItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InventoryItem::Grindstone => write!(f, "Grindstone"),
            InventoryItem::Shop => write!(f, "Shop"),
            InventoryItem::TownHall => write!(f, "Town Hall"),
        }
    }
}

/// Task that makes a miner path to a destination and perform an action on arrival.
#[derive(Clone)]
enum MinerTask {
    /// Walk to the time crystal to pick up the grindstone.
    GoToTimeCrystal,
    /// Carry the grindstone to an empty tile and place it.
    CarryGrindstoneToEmpty { dest: (usize, usize) },
    /// Walk to the placed grindstone to get a damage upgrade.
    GoToGrindstone,
    /// Walk to the time crystal to pick up the shop.
    GoToTimeCrystalForShop,
    /// Carry the shop to an empty tile and place it.
    CarryShopToEmpty { dest: (usize, usize) },
    /// Walk to the time crystal to pick up the town hall.
    GoToTimeCrystalForTownHall,
    /// Carry the town hall to an empty tile and place it.
    CarryTownHallToEmpty { dest: (usize, usize) },
}

#[derive(Clone)]
struct Miner {
    x: usize,
    y: usize,
    /// Home respawn slot around the time crystal for this miner.
    home_x: usize,
    home_y: usize,
    name: String,
    miner_type: String,
    hp: u32,
    max_hp: u32,
    mana: u32,
    max_mana: u32,
    movement_distance: u32,
    movement_type: String,
    color_name: String,
    color: egui::Color32,
    target: Option<(usize, usize)>,
    pick_damage: u8,
    /// Current pathfinding task; when Some, miner follows path instead of random mining.
    task: Option<MinerTask>,
    /// Path to follow; path[0] is current position, path.last() is destination.
    path: Vec<(usize, usize)>,
    /// Index into path of the next tile to step to (1 = first step).
    path_index: usize,
    /// Items the miner is carrying (e.g. Grindstone when fetching from time crystal).
    inventory: Vec<InventoryItem>,
    /// This miner's individual pathing style / AI.
    pathing_style: PathingStyle,
}

#[derive(Clone)]
struct Snapshot {
    world: World,
    game: GameState,
    miners: Vec<Miner>,
    harvested_ground: u32,
    harvested_zero: u32,
    harvested_q: u32,
    harvested_d: u32,
    harvested_g: u32,
    harvested_c: u32,
    step_counter: u64,
    grindstone_position: Option<(usize, usize)>,
    shop_position: Option<(usize, usize)>,
    town_hall_position: Option<(usize, usize)>,
    pathing_style: PathingStyle,
    pending_grindstones: u32,
    pending_shops: u32,
    pending_town_halls: u32,
}

struct App {
    world: World,
    game: GameState,
    last_tick_instant: Instant,
    last_save_instant: Instant,
    miners: Vec<Miner>,
    harvested_ground: u32,
    harvested_zero: u32,
    harvested_q: u32,
    harvested_d: u32,
    harvested_g: u32,
    harvested_c: u32,
    step_counter: u64,
    pending_grindstones: u32,
    pending_shops: u32,
    pending_town_halls: u32,
    selected_miner: Option<usize>,
    snapshots: Vec<Snapshot>,
    current_index: usize,
    paused: bool,
    active_panel: ActivePanel,
    camera_x: usize,
    camera_y: usize,
    camera_w: usize,
    camera_h: usize,
    /// When set, camera centers on this world tile (set by clicking the map).
    camera_focus_override: Option<(usize, usize)>,
    grindstone_position: Option<(usize, usize)>,
    shop_position: Option<(usize, usize)>,
    town_hall_position: Option<(usize, usize)>,
    last_manual_step_instant: Instant,
    camera_initialized: bool,
    pathing_style: PathingStyle,
    event_log: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Miners,
    TimeCrystal,
    Shop,
    Grindstone,
    TownHall,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathingStyle {
    Random,
    ClosestOreFromMiner,
    ClosestOreFromSpawn,
}

impl App {
    fn push_log(&mut self, entry: String) {
        const MAX_LOG: usize = 64;
        self.event_log.push(entry);
        if self.event_log.len() > MAX_LOG {
            let overflow = self.event_log.len() - MAX_LOG;
            self.event_log.drain(0..overflow);
        }
    }

    /// Find the nearest mining target relative to the time crystal (spawn).
    /// Uses the same logic as `find_nearest_mine_target` but always starts
    /// the search at the spawn position.
    fn find_spawn_mine_target(&self) -> Option<((usize, usize), (usize, usize))> {
        let origin = (self.world.spawn_x, self.world.spawn_y);
        self.find_nearest_mine_target(origin)
    }

    fn miner_color_for_index(index: usize) -> (String, egui::Color32) {
        use egui::Color32;
        let palette: &[(&str, Color32)] = &[
            ("White", Color32::WHITE),
            ("Green", Color32::GREEN),
            ("Blue", Color32::BLUE),
            ("Red", Color32::RED),
            ("Yellow", Color32::YELLOW),
            ("Cyan", Color32::from_rgb(0, 255, 255)),
            ("Magenta", Color32::from_rgb(255, 0, 255)),
        ];
        let (name, color) = palette[index % palette.len()];
        (name.to_string(), color)
    }

    fn new() -> Self {
        // Larger map for more room to roam and mine.
        let world_width = 120;
        let world_height = 60;
        let world = World::new(world_width, world_height);

        let game = game_core::load_or_new();

        let npc_x = world.spawn_x;
        let npc_y = world.spawn_y;

        // Find an empty tile around the time crystal to spawn the main miner and set home slot.
        let mut start_x = npc_x;
        let mut start_y = npc_y;
        'outer: for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = npc_x as isize + dx;
                let ny = npc_y as isize + dy;
                if nx >= 0
                    && nx < world_width as isize
                    && ny >= 0
                    && ny < world_height as isize
                {
                    let idx = world.idx(nx as usize, ny as usize);
                    if matches!(world.tiles[idx].tile, Tile::Empty) {
                        start_x = nx as usize;
                        start_y = ny as usize;
                        break 'outer;
                    }
                }
            }
        }

        let now_instant = Instant::now();

         // Initial miner stats.
        let mut rng = rand::thread_rng();
        let hp_dist = Uniform::new_inclusive(5u32, 10u32);
        let hp = rng.sample(hp_dist);
        let mana = rng.sample(hp_dist);

        let (koko_color_name, koko_color) = App::miner_color_for_index(0);

        let mut app = Self {
            world,
            game,
            last_tick_instant: now_instant,
            last_save_instant: now_instant,
            miners: vec![Miner {
                x: start_x,
                y: start_y,
                home_x: start_x,
                home_y: start_y,
                name: "Koko".to_string(),
                miner_type: "Water".to_string(),
                hp,
                max_hp: hp,
                mana,
                max_mana: mana,
                movement_distance: 1,
                movement_type: "Random".to_string(),
                color_name: koko_color_name,
                color: koko_color,
                target: None,
                pick_damage: 1,
                task: None,
                path: Vec::new(),
                path_index: 0,
                inventory: Vec::new(),
                pathing_style: PathingStyle::ClosestOreFromSpawn,
            }],
            harvested_ground: 0,
            harvested_zero: 0,
            harvested_q: 0,
            harvested_d: 0,
            harvested_g: 0,
            harvested_c: 0,
            step_counter: 0,
            pending_grindstones: 0,
            pending_shops: 0,
            pending_town_halls: 0,
            selected_miner: None,
            snapshots: Vec::new(),
            current_index: 0,
            paused: false,
            active_panel: ActivePanel::TimeCrystal,
            camera_x: 0,
            camera_y: 0,
            camera_w: world_width,
            camera_h: world_height,
            camera_focus_override: None,
            grindstone_position: None,
            shop_position: None,
            town_hall_position: None,
            last_manual_step_instant: now_instant,
            camera_initialized: false,
            pathing_style: PathingStyle::ClosestOreFromSpawn,
            event_log: Vec::new(),
        };

        app.capture_snapshot();
        app
    }

    fn capture_snapshot(&mut self) {
        // Maximum number of undo/redo snapshots to keep.
        // Raised from 256 so the game can run for many more years
        // without feeling like it "hits a limit".
        const MAX_HISTORY: usize = 4096;

        let snap = Snapshot {
            world: self.world.clone(),
            game: self.game.clone(),
            miners: self.miners.clone(),
            harvested_ground: self.harvested_ground,
            harvested_zero: self.harvested_zero,
            harvested_q: self.harvested_q,
            harvested_d: self.harvested_d,
            harvested_g: self.harvested_g,
            harvested_c: self.harvested_c,
            step_counter: self.step_counter,
            grindstone_position: self.grindstone_position,
            shop_position: self.shop_position,
            town_hall_position: self.town_hall_position,
            pathing_style: self.pathing_style,
            pending_grindstones: self.pending_grindstones,
            pending_shops: self.pending_shops,
            pending_town_halls: self.pending_town_halls,
        };

        // Drop any redo history beyond the current index.
        if self.current_index + 1 < self.snapshots.len() {
            self.snapshots.truncate(self.current_index + 1);
        }

        self.snapshots.push(snap);

        // Enforce max history size.
        if self.snapshots.len() > MAX_HISTORY {
            let overflow = self.snapshots.len() - MAX_HISTORY;
            self.snapshots.drain(0..overflow);
            if self.current_index >= overflow {
                self.current_index -= overflow;
            } else {
                self.current_index = 0;
            }
        } else {
            self.current_index = self.snapshots.len() - 1;
        }
    }

    fn restore_from_current(&mut self) {
        if let Some(snap) = self.snapshots.get(self.current_index).cloned() {
            self.world = snap.world;
            self.game = snap.game;
            self.miners = snap.miners;
            self.harvested_ground = snap.harvested_ground;
            self.harvested_zero = snap.harvested_zero;
            self.harvested_q = snap.harvested_q;
            self.harvested_d = snap.harvested_d;
            self.harvested_g = snap.harvested_g;
            self.harvested_c = snap.harvested_c;
            self.step_counter = snap.step_counter;
            self.grindstone_position = snap.grindstone_position;
            self.shop_position = snap.shop_position;
            self.town_hall_position = snap.town_hall_position;
            self.pathing_style = snap.pathing_style;
            self.pending_grindstones = snap.pending_grindstones;
            self.pending_shops = snap.pending_shops;
            self.pending_town_halls = snap.pending_town_halls;
        }
    }

    fn is_at_head(&self) -> bool {
        self.current_index + 1 == self.snapshots.len()
    }

    fn tick_game_if_needed(&mut self) {
        let now_instant = Instant::now();
        let since_last = now_instant.duration_since(self.last_tick_instant);

        // Run the core tick and NPC step at ~2.5 Hz to slow movement.
        if since_last >= Duration::from_millis(400) && !self.paused && self.is_at_head() {
            self.single_tick();
            self.last_tick_instant = now_instant;
        }
    }

    /// Carve a straight path (empty tiles) from spawn to the grindstone so it stays connected.
    fn carve_grindstone_path(&mut self) {
        let Some(gpos) = self.grindstone_position else {
            return;
        };
        let (sx, sy) = (self.world.spawn_x as i64, self.world.spawn_y as i64);
        let (gx, gy) = (gpos.0 as i64, gpos.1 as i64);
        let empty_cell = Cell {
            tile: Tile::Empty,
            max_durability: 0,
            durability: 0,
        };
        let steps = (gx - sx).abs().max((gy - sy).abs()).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = (sx as f64 + t * (gx - sx) as f64).round() as i64;
            let y = (sy as f64 + t * (gy - sy) as f64).round() as i64;
            // Make the path one tile thicker by clearing a 3x3 neighborhood around the line.
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx >= 0
                        && nx < self.world.width as i64
                        && ny >= 0
                        && ny < self.world.height as i64
                    {
                        let idx = self.world.idx(nx as usize, ny as usize);
                        self.world.tiles[idx] = empty_cell.clone();
                    }
                }
            }
        }
    }

    /// Carve a straight path (empty tiles) from spawn to the shop so it stays connected.
    fn carve_shop_path(&mut self) {
        let Some(spos) = self.shop_position else {
            return;
        };
        let (sx, sy) = (self.world.spawn_x as i64, self.world.spawn_y as i64);
        let (gx, gy) = (spos.0 as i64, spos.1 as i64);
        let empty_cell = Cell {
            tile: Tile::Empty,
            max_durability: 0,
            durability: 0,
        };
        let steps = (gx - sx).abs().max((gy - sy).abs()).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = (sx as f64 + t * (gx - sx) as f64).round() as i64;
            let y = (sy as f64 + t * (gy - sy) as f64).round() as i64;
            // Make the path one tile thicker by clearing a 3x3 neighborhood around the line.
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx >= 0
                        && nx < self.world.width as i64
                        && ny >= 0
                        && ny < self.world.height as i64
                    {
                        let idx = self.world.idx(nx as usize, ny as usize);
                        self.world.tiles[idx] = empty_cell.clone();
                    }
                }
            }
        }
    }

    /// Reset the world and NPC position while keeping accumulated resources.
    fn reset_run(&mut self) {
        let world_width = self.world.width;
        let world_height = self.world.height;
        self.world = World::new(world_width, world_height);

        // If the grindstone or shop were placed, keep their positions and carve paths
        // from the time crystal to them. (positions are not cleared)
        self.carve_grindstone_path();
        self.carve_shop_path();

        for miner in &mut self.miners {
            // Respawn each miner at their personal home slot around the time crystal.
            miner.x = miner.home_x.min(self.world.width.saturating_sub(1));
            miner.y = miner.home_y.min(self.world.height.saturating_sub(1));
            miner.target = None;
            miner.task = None;
            miner.path.clear();
            miner.path_index = 0;
            // Keep inventory (e.g. grindstone) across resets
        }

        self.step_counter = 0;
        self.camera_initialized = false;
        self.camera_focus_override = None;
    }

    fn generate_miner_name(&self, rng: &mut impl Rng) -> String {
        const ADJECTIVES: &[&str] = &[
            "Brave", "Clever", "Greedy", "Silent", "Swift", "Lucky", "Rusty", "Shiny", "Grumpy",
            "Cheerful",
        ];
        const NOUNS: &[&str] = &[
            "Pickaxe", "Helmet", "Dwarf", "Goblin", "Mole", "Canary", "Drill", "Cart", "Lantern",
            "Tunnel",
        ];

        let adj = ADJECTIVES.choose(rng).unwrap_or(&"Nameless");
        let noun = NOUNS.choose(rng).unwrap_or(&"Miner");
        format!("{} {}", adj, noun)
    }

    /// Tile is walkable for pathfinding: Empty, time crystal (spawn), grindstone, shop, or town hall.
    fn is_walkable(&self, x: usize, y: usize) -> bool {
        if x >= self.world.width || y >= self.world.height {
            return false;
        }
        if x == self.world.spawn_x && y == self.world.spawn_y {
            return true;
        }
        if self.grindstone_position == Some((x, y)) {
            return true;
        }
        if self.shop_position == Some((x, y)) {
            return true;
        }
        if self.town_hall_position == Some((x, y)) {
            return true;
        }
        matches!(
            self.world.tiles[self.world.idx(x, y)].tile,
            Tile::Empty | Tile::Path
        )
    }

    /// Draw a path ('.' tiles) from the time crystal to a building position using a line.
    fn place_path_from_spawn_to(&mut self, dest: (usize, usize)) {
        let x0 = self.world.spawn_x as isize;
        let y0 = self.world.spawn_y as isize;
        let x1 = dest.0 as isize;
        let y1 = dest.1 as isize;
        let w = self.world.width as isize;
        let h = self.world.height as isize;

        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            let (ux, uy) = (x as usize, y as usize);
            if (x, y) != (x1, y1) && x >= 0 && x < w && y >= 0 && y < h {
                let idx = self.world.idx(ux, uy);
                self.world.tiles[idx].tile = Tile::Path;
                self.world.tiles[idx].max_durability = 0;
                self.world.tiles[idx].durability = 0;
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// BFS from start to goal; returns path including start and goal, or None if unreachable.
    fn find_path(&self, start: (usize, usize), goal: (usize, usize)) -> Option<Vec<(usize, usize)>> {
        if start == goal {
            return Some(vec![start]);
        }
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut parent: std::collections::HashMap<(usize, usize), (usize, usize)> =
            std::collections::HashMap::new();
        queue.push_back(start);

        while let Some(p) = queue.pop_front() {
            if p == goal {
                let mut path = vec![goal];
                let mut cur = goal;
                while let Some(&prev) = parent.get(&cur) {
                    path.push(prev);
                    cur = prev;
                    if cur == start {
                        break;
                    }
                }
                path.reverse();
                return Some(path);
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) || parent.contains_key(&n) {
                    continue;
                }
                parent.insert(n, p);
                queue.push_back(n);
            }
        }
        None
    }

    /// Nearest empty tile (by steps from `start`) that is at least 7 tiles away from the time crystal.
    /// BFS starts at the miner holding the grindstone so they place it near themselves, but only on
    /// tiles that are far enough from the time crystal.
    fn find_grindstone_place_from(
        &self,
        start: (usize, usize),
    ) -> Option<(usize, usize)> {
        const MIN_TILES_FROM_SPAWN: isize = 7;
        let spawn = (self.world.spawn_x, self.world.spawn_y);
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(start);
        seen.insert(start);

        while let Some(p) = queue.pop_front() {
            if p != start
                && self.world.idx(p.0, p.1) < self.world.tiles.len()
                && self.grindstone_position != Some(p)
            {
                let dx = p.0 as isize - spawn.0 as isize;
                let dy = p.1 as isize - spawn.1 as isize;
                let manhattan = dx.abs() + dy.abs();
                if manhattan >= MIN_TILES_FROM_SPAWN {
                    let cell = &self.world.tiles[self.world.idx(p.0, p.1)];
                    if matches!(cell.tile, Tile::Empty) {
                        return Some(p);
                    }
                }
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height || seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                seen.insert(n);
                queue.push_back(n);
            }
        }
        None
    }

    /// Nearest empty tile (by steps from `start`) that is at least 4 tiles away from the time crystal,
    /// used to place the shop.
    fn find_shop_place_from(&self, start: (usize, usize)) -> Option<(usize, usize)> {
        const MIN_TILES_FROM_SPAWN: isize = 4;
        let spawn = (self.world.spawn_x, self.world.spawn_y);
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(start);
        seen.insert(start);

        while let Some(p) = queue.pop_front() {
            if p != start
                && self.world.idx(p.0, p.1) < self.world.tiles.len()
                && self.shop_position != Some(p)
            {
                let dx = p.0 as isize - spawn.0 as isize;
                let dy = p.1 as isize - spawn.1 as isize;
                let manhattan = dx.abs() + dy.abs();
                if manhattan >= MIN_TILES_FROM_SPAWN {
                    let cell = &self.world.tiles[self.world.idx(p.0, p.1)];
                    if matches!(cell.tile, Tile::Empty) {
                        return Some(p);
                    }
                }
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height || seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                seen.insert(n);
                queue.push_back(n);
            }
        }
        None
    }

    /// Nearest empty tile that is at least 10 tiles (Manhattan distance) from the time crystal,
    /// used to place the town hall.
    fn find_town_hall_place_from(&self, start: (usize, usize)) -> Option<(usize, usize)> {
        const MIN_TILES_FROM_SPAWN: isize = 10;
        let spawn = (self.world.spawn_x, self.world.spawn_y);
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(start);
        seen.insert(start);

        while let Some(p) = queue.pop_front() {
            if p != start
                && self.world.idx(p.0, p.1) < self.world.tiles.len()
                && self.town_hall_position != Some(p)
            {
                let dx = p.0 as isize - spawn.0 as isize;
                let dy = p.1 as isize - spawn.1 as isize;
                let manhattan = dx.abs() + dy.abs();
                if manhattan >= MIN_TILES_FROM_SPAWN {
                    let cell = &self.world.tiles[self.world.idx(p.0, p.1)];
                    if matches!(cell.tile, Tile::Empty) {
                        return Some(p);
                    }
                }
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height || seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                seen.insert(n);
                queue.push_back(n);
            }
        }
        None
    }

    /// For "nearest wall" pathing: from `start`, find the next step along a shortest path
    /// toward the closest minable tile (ground or ore). Returns the immediate next (nx, ny)
    /// to move into, or None if no such tile is reachable.
    fn next_step_towards_nearest_wall(&self, start: (usize, usize)) -> Option<(usize, usize)> {
        use std::collections::{HashMap, VecDeque};

        let mut queue = VecDeque::new();
        let mut parent: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
        queue.push_back(start);

        let mut seen = std::collections::HashSet::new();
        seen.insert(start);

        let in_bounds = |x: isize, y: isize| {
            x >= 0
                && y >= 0
                && (x as usize) < self.world.width
                && (y as usize) < self.world.height
        };

        while let Some(p) = queue.pop_front() {
            let (px, py) = p;

            // If any neighbor of p is a minable tile, treat that neighbor as the goal.
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = px as isize + dx;
                let ny = py as isize + dy;
                if !in_bounds(nx, ny) {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                let idx = self.world.idx(n.0, n.1);
                let cell = &self.world.tiles[idx];
                if matches!(cell.tile, Tile::Ground | Tile::Ore(_)) {
                    // Found closest minable neighbor; reconstruct path from start to p,
                    // then step either directly into the wall (if already adjacent),
                    // or along the path toward it.
                    let mut path = vec![p];
                    let mut cur = p;
                    while let Some(&prev) = parent.get(&cur) {
                        path.push(prev);
                        cur = prev;
                        if cur == start {
                            break;
                        }
                    }
                    path.reverse();
                    // path[0] should be start. If path has another step, use that.
                    if path.len() > 1 {
                        return Some(path[1]);
                    } else {
                        // Already adjacent to the wall: step directly into it.
                        return Some(n);
                    }
                }
            }

            // Explore neighbors that are walkable (empty, spawn, buildings).
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = px as isize + dx;
                let ny = py as isize + dy;
                if !in_bounds(nx, ny) {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                parent.insert(n, p);
                seen.insert(n);
                queue.push_back(n);
            }
        }

        None
    }

    /// Find the nearest mining target: a walkable tile to stand on, plus an adjacent
    /// minable tile (ground or ore) that can be worked on from there.
    /// Returns (walk_position, target_tile_to_mine).
    fn find_nearest_mine_target(
        &self,
        origin: (usize, usize),
    ) -> Option<((usize, usize), (usize, usize))> {
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(origin);
        seen.insert(origin);

        while let Some(p) = queue.pop_front() {
            // For each walkable tile, look for an adjacent minable tile.
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0
                    || ny < 0
                    || nx >= self.world.width as isize
                    || ny >= self.world.height as isize
                {
                    continue;
                }
                let tx = nx as usize;
                let ty = ny as usize;
                let idx = self.world.idx(tx, ty);
                let cell = &self.world.tiles[idx];
                if matches!(cell.tile, Tile::Ground | Tile::Ore(_)) && cell.durability > 0 {
                    // We can stand on p and mine (tx, ty).
                    return Some((p, (tx, ty)));
                }
            }

            // BFS expansion over walkable tiles.
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height || seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                seen.insert(n);
                queue.push_back(n);
            }
        }

        None
    }

    /// Collect up to `limit` mining targets from `origin`, each as (walk, target, depth).
    /// Depth is BFS steps from origin to the walk tile; used for weighted random (closer = higher weight).
    fn find_nearest_mine_targets(
        &self,
        origin: (usize, usize),
        limit: usize,
    ) -> Vec<((usize, usize), (usize, usize), u32)> {
        use std::collections::{HashSet, VecDeque};
        let mut queue = VecDeque::new();
        let mut seen = HashSet::new();
        let mut results = Vec::with_capacity(limit);
        let mut seen_target = HashSet::new();
        queue.push_back((origin, 0u32));
        seen.insert(origin);

        while let Some((p, depth)) = queue.pop_front() {
            if results.len() >= limit {
                break;
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0
                    || ny < 0
                    || nx >= self.world.width as isize
                    || ny >= self.world.height as isize
                {
                    continue;
                }
                let tx = nx as usize;
                let ty = ny as usize;
                let idx = self.world.idx(tx, ty);
                let cell = &self.world.tiles[idx];
                if matches!(cell.tile, Tile::Ground | Tile::Ore(_)) && cell.durability > 0 {
                    if seen_target.insert((tx, ty)) {
                        results.push((p, (tx, ty), depth));
                    }
                }
            }
            for (dx, dy) in [(-1isize, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = p.0 as isize + dx;
                let ny = p.1 as isize + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if n.0 >= self.world.width || n.1 >= self.world.height || seen.contains(&n) {
                    continue;
                }
                if !self.is_walkable(n.0, n.1) {
                    continue;
                }
                seen.insert(n);
                queue.push_back((n, depth + 1));
            }
        }
        results
    }

    /// Multiple mining targets from spawn (time crystal), for weighted random per miner.
    fn find_spawn_mine_targets(&self, limit: usize) -> Vec<((usize, usize), (usize, usize), u32)> {
        let origin = (self.world.spawn_x, self.world.spawn_y);
        self.find_nearest_mine_targets(origin, limit)
    }

    fn toggle_pause(&mut self) {
        if self.paused && !self.is_at_head() {
            // When resuming from a past point, drop future history
            // so we can branch and keep ticking forward normally.
            if self.current_index + 1 < self.snapshots.len() {
                self.snapshots.truncate(self.current_index + 1);
            }
        }
        self.paused = !self.paused;
    }

    /// Base color for each ore (full brightness); darkened by durability in build_colored_ascii.
    fn ore_color_base(kind: OreKind) -> (u8, u8, u8) {
        match kind {
            OreKind::Zero => (120, 200, 255), // soft cyan
            OreKind::Q => (255, 212, 120),    // warm gold
            OreKind::D => (255, 160, 100),    // amber
            OreKind::G => (120, 220, 150),    // sage green
            OreKind::C => (200, 140, 255),    // violet
        }
    }

    fn ore_color(kind: OreKind, max_durability: u8) -> egui::Color32 {
        use eframe::egui::Color32;
        let (r, g, b) = Self::ore_color_base(kind);
        if max_durability == 0 {
            return Color32::from_rgb(r, g, b);
        }
        // Same gradient as stone: lighter at center (low durability), darker at edges (high).
        let t = (max_durability.saturating_sub(1)) as f32 / 19.0;
        let base_gray = (255.0 * (1.0 - t)).round().clamp(0.0, 255.0);
        let gray = (base_gray - 40.0).max(0.0);
        let factor = gray / 255.0;
        let r2 = (r as f32 * factor).round().clamp(0.0, 255.0) as u8;
        let g2 = (g as f32 * factor).round().clamp(0.0, 255.0) as u8;
        let b2 = (b as f32 * factor).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(r2, g2, b2)
    }

    fn build_colored_ascii(&self, ascii: &str) -> egui::text::LayoutJob {
        use eframe::egui::text::LayoutJob;
        use eframe::egui::{Color32, FontId, TextFormat};

        let mut job = LayoutJob::default();

        let mut x = 0usize;
        let mut y = 0usize;

        for ch in ascii.chars() {
            if ch == '\n' {
                job.append(
                    "\n",
                    0.0,
                    TextFormat {
                        font_id: FontId::monospace(14.0),
                        color: Color32::WHITE,
                        ..Default::default()
                    },
                );
                x = 0;
                y += 1;
                continue;
            }

            let mut format = TextFormat {
                font_id: FontId::monospace(14.0),
                color: Color32::WHITE,
                ..Default::default()
            };

            // Map viewport coords (x, y) back into world coords to look up durability.
            if y < self.camera_h {
                let wx = self.camera_x.saturating_add(x);
                let wy = self.camera_y.saturating_add(y);
                if wx < self.world.width && wy < self.world.height {
                    let idx = self.world.idx(wx, wy);
                    let cell = &self.world.tiles[idx];
                    match cell.tile {
                        Tile::Ore(kind) => {
                            format.color = Self::ore_color(kind, cell.max_durability);
                        }
                        Tile::Path => {
                            format.color = Color32::from_rgb(200, 180, 140);
                        }
                        Tile::Ground | Tile::Empty => {
                            if cell.max_durability > 0 {
                                // Map durability 1..20 to gray (white -> soft, black -> hard).
                                let t = (cell.max_durability.saturating_sub(1)) as f32 / 19.0;
                                let base_gray =
                                    (255.0 * (1.0 - t)).round().clamp(0.0, 255.0) as u8;
                                let gray = base_gray.saturating_sub(40);
                                format.color = Color32::from_gray(gray);
                            }
                        }
                    }

                    if self.grindstone_position == Some((wx, wy)) {
                        format.color = Color32::from_rgb(180, 150, 120);
                    }
                    if self.shop_position == Some((wx, wy)) {
                        format.color = Color32::from_rgb(200, 200, 80);
                    }
                    if self.town_hall_position == Some((wx, wy)) {
                        format.color = Color32::from_rgb(180, 210, 255);
                    }

                    // Use miner-specific color if there is a miner or home marker on this tile.
                    let mut colored = false;
                    for miner in &self.miners {
                        if miner.x == wx && miner.y == wy {
                            format.color = miner.color;
                            colored = true;
                            break;
                        }
                    }
                    if !colored {
                        for miner in &self.miners {
                            if miner.home_x == wx && miner.home_y == wy {
                                format.color = miner.color;
                                break;
                            }
                        }
                    }
                }
            }

            let s = ch.to_string();
            job.append(&s, 0.0, format);
            x += 1;
        }

        job
    }

    fn autosave_if_needed(&mut self) {
        let now_instant = Instant::now();
        let since_last = now_instant.duration_since(self.last_save_instant);
        if since_last >= Duration::from_secs(5) {
            if let Err(e) = game_core::save_game(&self.game) {
                eprintln!("Failed to save game: {e}");
            }
            self.last_save_instant = now_instant;
        }
    }

    /// Move the NPC one step in a random cardinal direction.
    /// If they have a pathfinding task, follow the path instead; on arrival run the task action.
    fn step_npc_random(&mut self) {
        if self.miners.is_empty() {
            return;
        }

        let mut rng = rand::thread_rng();
        let mut arrivals: Vec<(usize, Option<MinerTask>)> = Vec::new();
        let mut path_requests: Vec<(usize, (usize, usize), (usize, usize))> = Vec::new();
        let mut spawn_mine_requests: Vec<(usize, (usize, usize), (usize, usize), (usize, usize))> =
            Vec::new();
        let mut place_town_hall_from: Vec<(usize, (usize, usize))> = Vec::new();

        // Assign pending building orders (grindstone, shop, town hall) to idle miners.
        // Each order becomes a "go to time crystal to pick up X" task.
        while self.pending_grindstones > 0 {
            if let Some((idx, _)) = self.miners.iter().enumerate().find(|(_, m)| {
                m.task.is_none()
                    && !m.inventory.iter().any(|it| matches!(it, InventoryItem::Grindstone))
            }) {
                self.miners[idx].task = Some(MinerTask::GoToTimeCrystal);
                self.miners[idx].path.clear();
                self.miners[idx].path_index = 0;
                self.pending_grindstones -= 1;
            } else {
                break;
            }
        }
        while self.pending_shops > 0 {
            if let Some((idx, _)) = self.miners.iter().enumerate().find(|(_, m)| {
                m.task.is_none()
                    && !m.inventory.iter().any(|it| matches!(it, InventoryItem::Shop))
            }) {
                self.miners[idx].task = Some(MinerTask::GoToTimeCrystalForShop);
                self.miners[idx].path.clear();
                self.miners[idx].path_index = 0;
                self.pending_shops -= 1;
            } else {
                break;
            }
        }
        while self.pending_town_halls > 0 {
            if let Some((idx, _)) = self.miners.iter().enumerate().find(|(_, m)| {
                m.task.is_none()
                    && !m.inventory.iter().any(|it| matches!(it, InventoryItem::TownHall))
            }) {
                self.miners[idx].task = Some(MinerTask::GoToTimeCrystalForTownHall);
                self.miners[idx].path.clear();
                self.miners[idx].path_index = 0;
                self.pending_town_halls -= 1;
            } else {
                break;
            }
        }

        // To keep borrowing simple, we work in two phases:
        // 1) Decide miner actions and collect any pathfinding requests that need &self.
        // 2) Apply those requests after the loop.
        // For nearest-wall AI, we queue (miner_index, origin_position) pairs here,
        // and we also precompute a spawn-centric mining target for "closest from time crystal".
        let mut mine_path_requests: Vec<(usize, (usize, usize))> = Vec::new();
        let spawn_candidates = self.find_spawn_mine_targets(40);

        for (i, miner) in self.miners.iter_mut().enumerate() {
            // Pathfinding task: follow path or request one, then possibly arrive.
            if let Some(ref task) = miner.task {
                let goal = match task {
                    MinerTask::GoToTimeCrystal
                    | MinerTask::GoToTimeCrystalForShop
                    | MinerTask::GoToTimeCrystalForTownHall => {
                        (self.world.spawn_x, self.world.spawn_y)
                    }
                    MinerTask::CarryGrindstoneToEmpty { dest } => *dest,
                    MinerTask::CarryShopToEmpty { dest } => *dest,
                    MinerTask::CarryTownHallToEmpty { dest } => *dest,
                    MinerTask::GoToGrindstone => match self.grindstone_position {
                        Some(pos) => pos,
                        None => {
                            miner.task = None;
                            miner.path.clear();
                            miner.path_index = 0;
                            continue;
                        }
                    },
                };

                if miner.path.is_empty() {
                    path_requests.push((i, (miner.x, miner.y), goal));
                    continue;
                }

                if miner.path_index + 1 < miner.path.len() {
                    miner.path_index += 1;
                    let next = miner.path[miner.path_index];
                    miner.x = next.0;
                    miner.y = next.1;
                    miner.target = None;
                } else {
                    let arrived = *miner.path.last().unwrap_or(&(miner.x, miner.y));
                    miner.x = arrived.0;
                    miner.y = arrived.1;
                    miner.path.clear();
                    miner.path_index = 0;
                    arrivals.push((i, std::mem::take(&mut miner.task)));
                }
                continue;
            }

            // No task: AI-driven mining movement.
            // If this miner has a current target tile, keep working on it
            // until it breaks. Otherwise, path to the nearest minable wall when
            // "nearest wall" AI is active, or fall back to random wandering.

            // If we already have a local path (for nearest-wall AI), follow it.
            if miner.path_index + 1 < miner.path.len() {
                miner.path_index += 1;
                let next = miner.path[miner.path_index];
                miner.x = next.0;
                miner.y = next.1;
                // Keep existing miner.target (the wall to mine) if any.
                continue;
            } else if !miner.path.is_empty() {
                // Reached the end of the path; clear it so we start mining the target.
                miner.path.clear();
                miner.path_index = 0;
            }

            let (nx, ny) = if let Some((tx, ty)) = miner.target {
                (tx, ty)
            } else if matches!(miner.pathing_style, PathingStyle::ClosestOreFromSpawn) {
                // Pick a random minable tile weighted by distance to time crystal (closer = more likely).
                if !spawn_candidates.is_empty() {
                    let weights: Vec<f32> = spawn_candidates
                        .iter()
                        .map(|(_, _, d)| 1.0 / (1.0 + *d as f32))
                        .collect();
                    if let Ok(dist) = WeightedIndex::new(&weights) {
                        let idx = dist.sample(&mut rng);
                        let (walk, target, _) = spawn_candidates[idx];
                        spawn_mine_requests.push((i, (miner.x, miner.y), walk, target));
                        continue;
                    }
                }
                // If no candidates, fall back to random wandering.
                let dir = rng.gen_range(0..4);
                let (dx, dy) = match dir {
                    0 => (-1isize, 0isize), // left
                    1 => (1, 0),            // right
                    2 => (0, -1),           // up
                    _ => (0, 1),            // down
                };

                let mut new_x = miner.x as isize + dx;
                let new_y = miner.y as isize + dy;

                // Wrap horizontally on the bottom row to make it circular.
                let bottom_y = self.world.bottom_y() as isize;
                if new_y == bottom_y {
                    if new_x < 0 {
                        new_x = self.world.width as isize - 1;
                    } else if new_x >= self.world.width as isize {
                        new_x = 0;
                    }
                }

                // Bounds check.
                if new_x < 0
                    || new_y < 0
                    || new_x >= self.world.width as isize
                    || new_y >= self.world.height as isize
                {
                    continue;
                }

                (new_x as usize, new_y as usize)
            } else if matches!(miner.pathing_style, PathingStyle::ClosestOreFromMiner) {
                // "Nearest wall": enqueue a request from this miner's current position
                // to path toward the closest minable tile. This tick still uses random
                // movement; the computed path will be applied after the loop.
                mine_path_requests.push((i, (miner.x, miner.y)));

                let dir = rng.gen_range(0..4);
                let (dx, dy) = match dir {
                    0 => (-1isize, 0isize), // left
                    1 => (1, 0),            // right
                    2 => (0, -1),           // up
                    _ => (0, 1),            // down
                };

                let mut new_x = miner.x as isize + dx;
                let new_y = miner.y as isize + dy;

                // Wrap horizontally on the bottom row to make it circular.
                let bottom_y = self.world.bottom_y() as isize;
                if new_y == bottom_y {
                    if new_x < 0 {
                        new_x = self.world.width as isize - 1;
                    } else if new_x >= self.world.width as isize {
                        new_x = 0;
                    }
                }

                // Bounds check.
                if new_x < 0
                    || new_y < 0
                    || new_x >= self.world.width as isize
                    || new_y >= self.world.height as isize
                {
                    continue;
                }

                (new_x as usize, new_y as usize)
            } else {
                // Random wandering (default AI).
                let dir = rng.gen_range(0..4);
                let (dx, dy) = match dir {
                    0 => (-1isize, 0isize), // left
                    1 => (1, 0),            // right
                    2 => (0, -1),           // up
                    _ => (0, 1),            // down
                };

                let mut new_x = miner.x as isize + dx;
                let new_y = miner.y as isize + dy;

                // Wrap horizontally on the bottom row to make it circular.
                let bottom_y = self.world.bottom_y() as isize;
                if new_y == bottom_y {
                    if new_x < 0 {
                        new_x = self.world.width as isize - 1;
                    } else if new_x >= self.world.width as isize {
                        new_x = 0;
                    }
                }

                // Bounds check.
                if new_x < 0
                    || new_y < 0
                    || new_x >= self.world.width as isize
                    || new_y >= self.world.height as isize
                {
                    continue;
                }

                (new_x as usize, new_y as usize)
            };
            let idx = self.world.idx(nx, ny);
            let cell = &mut self.world.tiles[idx];

            // If we step onto a ground or ore tile, damage it. When durability
            // reaches zero, it breaks and yields resources and we move in.
            match cell.tile {
                Tile::Ground => {
                    if cell.durability > 0 {
                        let before = cell.durability;
                        cell.durability =
                            cell.durability.saturating_sub(miner.pick_damage);
                        let after = cell.durability;
                        if after == 0 {
                            cell.tile = Tile::Empty;
                            self.harvested_ground =
                                self.harvested_ground.saturating_add(1);
                            miner.x = nx;
                            miner.y = ny;
                            miner.target = None;
                        } else {
                            miner.target = Some((nx, ny));
                        }
                        // Log will be added after the borrow ends.
                        self.event_log.push(format!(
                            "{} digs at wall ({}/{})",
                            miner.name, after, before
                        ));
                    } else {
                        // Already broken, just move in.
                        miner.x = nx;
                        miner.y = ny;
                        miner.target = None;
                    }
                }
                Tile::Ore(kind) => {
                    if cell.durability > 0 {
                        let before = cell.durability;
                        cell.durability =
                            cell.durability.saturating_sub(miner.pick_damage);
                        let after = cell.durability;
                        if after == 0 {
                            cell.tile = Tile::Empty;
                            match kind {
                                OreKind::Zero => {
                                    self.harvested_zero =
                                        self.harvested_zero.saturating_add(1);
                                }
                                OreKind::Q => {
                                    self.harvested_q =
                                        self.harvested_q.saturating_add(1);
                                }
                                OreKind::D => {
                                    self.harvested_d =
                                        self.harvested_d.saturating_add(1);
                                }
                                OreKind::G => {
                                    self.harvested_g =
                                        self.harvested_g.saturating_add(1);
                                }
                                OreKind::C => {
                                    self.harvested_c =
                                        self.harvested_c.saturating_add(1);
                                }
                            }
                            miner.x = nx;
                            miner.y = ny;
                            miner.target = None;
                        } else {
                            miner.target = Some((nx, ny));
                        }
                        let ore_name = match kind {
                            OreKind::Zero => "Zero",
                            OreKind::Q => "Q",
                            OreKind::D => "D",
                            OreKind::G => "G",
                            OreKind::C => "C",
                        };
                        self.event_log.push(format!(
                            "{} digs {ore_name} ({}/{})",
                            miner.name, after, before
                        ));
                    } else {
                        miner.x = nx;
                        miner.y = ny;
                        miner.target = None;
                    }
                }
                Tile::Empty | Tile::Path => {
                    miner.x = nx;
                    miner.y = ny;
                    miner.target = None;
                }
            }
        }

        // Resolve path requests (avoids borrowing self while iterating miners).
        for (i, start, goal) in path_requests {
            if let Some(path) = self.find_path(start, goal) {
                let len = path.len();
                self.miners[i].path = path;
                self.miners[i].path_index = 0;
                if len <= 1 {
                    arrivals.push((i, self.miners[i].task.take()));
                    self.miners[i].path.clear();
                }
            } else {
                self.miners[i].task = None;
            }
        }

        // Resolve spawn-based mining: each miner got a weighted-random (walk, target); path to walk and set target.
        for (i, origin, walk, target) in spawn_mine_requests {
            if i >= self.miners.len() {
                continue;
            }
            if let Some(path) = self.find_path(origin, walk) {
                let miner = &mut self.miners[i];
                miner.path = path;
                miner.path_index = 0;
                miner.target = Some(target);
            }
        }

        // Process task arrivals (set grindstone/shop position, inventory, next task).
        let mut place_grindstone_from: Vec<(usize, (usize, usize))> = Vec::new();
        let mut place_shop_from: Vec<(usize, (usize, usize))> = Vec::new();
        for (i, completed_task) in arrivals {
            match completed_task {
                Some(MinerTask::GoToTimeCrystal) => {
                    self.miners[i].inventory.push(InventoryItem::Grindstone);
                    place_grindstone_from.push((i, (self.miners[i].x, self.miners[i].y)));
                }
                Some(MinerTask::CarryGrindstoneToEmpty { dest }) => {
                    if let Some(pos) = self.miners[i]
                        .inventory
                        .iter()
                        .position(|item| matches!(item, InventoryItem::Grindstone))
                    {
                        self.miners[i].inventory.remove(pos);
                    }
                    self.grindstone_position = Some(dest);
                    self.place_path_from_spawn_to(dest);
                }
                Some(MinerTask::GoToGrindstone) => {
                    let d = self.miners[i].pick_damage;
                    self.miners[i].pick_damage = d.saturating_add(1).min(5);
                }
                Some(MinerTask::GoToTimeCrystalForShop) => {
                    self.miners[i].inventory.push(InventoryItem::Shop);
                    place_shop_from.push((i, (self.miners[i].x, self.miners[i].y)));
                }
                Some(MinerTask::CarryShopToEmpty { dest }) => {
                    if let Some(pos) = self.miners[i]
                        .inventory
                        .iter()
                        .position(|item| matches!(item, InventoryItem::Shop))
                    {
                        self.miners[i].inventory.remove(pos);
                    }
                    self.shop_position = Some(dest);
                    self.place_path_from_spawn_to(dest);
                }
                Some(MinerTask::GoToTimeCrystalForTownHall) => {
                    self.miners[i].inventory.push(InventoryItem::TownHall);
                    place_town_hall_from.push((i, (self.miners[i].x, self.miners[i].y)));
                }
                Some(MinerTask::CarryTownHallToEmpty { dest }) => {
                    if let Some(pos) = self.miners[i]
                        .inventory
                        .iter()
                        .position(|item| matches!(item, InventoryItem::TownHall))
                    {
                        self.miners[i].inventory.remove(pos);
                    }
                    self.town_hall_position = Some(dest);
                    self.place_path_from_spawn_to(dest);
                }
                None => {}
            }
        }
        for (i, pos) in place_grindstone_from {
            if let Some(dest) = self.find_grindstone_place_from(pos) {
                if let Some(path) = self.find_path(pos, dest) {
                    self.miners[i].path = path;
                    self.miners[i].path_index = 0;
                    self.miners[i].task = Some(MinerTask::CarryGrindstoneToEmpty { dest });
                }
            }
            // If no valid spot was found yet, miner keeps grindstone in inventory.
        }

        for (i, pos) in place_shop_from {
            if let Some(dest) = self.find_shop_place_from(pos) {
                if let Some(path) = self.find_path(pos, dest) {
                    self.miners[i].path = path;
                    self.miners[i].path_index = 0;
                    self.miners[i].task = Some(MinerTask::CarryShopToEmpty { dest });
                }
            }
            // If no valid spot was found yet, miner keeps shop in inventory.
        }

        for (i, pos) in place_town_hall_from {
            if let Some(dest) = self.find_town_hall_place_from(pos) {
                if let Some(path) = self.find_path(pos, dest) {
                    self.miners[i].path = path;
                    self.miners[i].path_index = 0;
                    self.miners[i].task = Some(MinerTask::CarryTownHallToEmpty { dest });
                }
            }
            // If no valid spot was found yet, miner keeps town hall in inventory.
        }

        // Resolve nearest-wall mining: pick a random target weighted by distance (closer = more likely).
        for (i, origin) in mine_path_requests {
            if i >= self.miners.len() {
                continue;
            }
            let candidates = self.find_nearest_mine_targets(origin, 15);
            if candidates.is_empty() {
                continue;
            }
            let weights: Vec<f32> = candidates
                .iter()
                .map(|(_, _, d)| 1.0 / (1.0 + *d as f32))
                .collect();
            if let Ok(dist) = WeightedIndex::new(&weights) {
                let mut rng = rand::thread_rng();
                let idx = dist.sample(&mut rng);
                let (walk, target, _) = candidates[idx];
                if let Some(path) = self.find_path(origin, walk) {
                    let miner = &mut self.miners[i];
                    miner.path = path;
                    miner.path_index = 0;
                    miner.target = Some(target);
                }
            }
        }

        // Each tick, if the grindstone is not yet placed and a miner is holding it,
        // let that miner look for the closest valid placement spot from their current position.
        if self.grindstone_position.is_none() {
            let miner_count = self.miners.len();
            for i in 0..miner_count {
                if self.miners[i]
                    .inventory
                    .iter()
                    .any(|it| matches!(it, InventoryItem::Grindstone))
                    && self.miners[i].task.is_none()
                {
                    let start = (self.miners[i].x, self.miners[i].y);
                    if let Some(dest) = self.find_grindstone_place_from(start) {
                        if let Some(path) = self.find_path(start, dest) {
                            self.miners[i].path = path;
                            self.miners[i].path_index = 0;
                            self.miners[i].task =
                                Some(MinerTask::CarryGrindstoneToEmpty { dest });
                        }
                    }
                }
            }
        }

        // Similarly, if the shop is not yet placed and a miner is holding it, try to place it.
        if self.shop_position.is_none() {
            let miner_count = self.miners.len();
            for i in 0..miner_count {
                if self.miners[i]
                    .inventory
                    .iter()
                    .any(|it| matches!(it, InventoryItem::Shop))
                    && self.miners[i].task.is_none()
                {
                    let start = (self.miners[i].x, self.miners[i].y);
                    if let Some(dest) = self.find_shop_place_from(start) {
                        if let Some(path) = self.find_path(start, dest) {
                            self.miners[i].path = path;
                            self.miners[i].path_index = 0;
                            self.miners[i].task = Some(MinerTask::CarryShopToEmpty { dest });
                        }
                    }
                }
            }
        }

        // If the town hall is not yet placed and a miner is holding it, try to place it.
        if self.town_hall_position.is_none() {
            let miner_count = self.miners.len();
            for i in 0..miner_count {
                if self.miners[i]
                    .inventory
                    .iter()
                    .any(|it| matches!(it, InventoryItem::TownHall))
                    && self.miners[i].task.is_none()
                {
                    let start = (self.miners[i].x, self.miners[i].y);
                    if let Some(dest) = self.find_town_hall_place_from(start) {
                        if let Some(path) = self.find_path(start, dest) {
                            self.miners[i].path = path;
                            self.miners[i].path_index = 0;
                            self.miners[i].task = Some(MinerTask::CarryTownHallToEmpty { dest });
                        }
                    }
                }
            }
        }

        // Advance the step timer; runs can now continue indefinitely until manually reset.
        self.step_counter = self.step_counter.saturating_add(1);
    }

    fn single_tick(&mut self) {
        let now = SystemTime::now();
        self.game.tick(now);
        self.step_npc_random();
        self.capture_snapshot();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick_game_if_needed();
        self.autosave_if_needed();

        let mut do_step_back = false;
        let mut do_step_forward = false;
        let mut toggle_pause_requested = false;

        // Manual step cooldown matched to the automatic tick interval.
        let now_instant = Instant::now();
        let manual_cooldown = Duration::from_millis(400);
        let since_manual = now_instant.duration_since(self.last_manual_step_instant);
        let can_manual_step = since_manual >= manual_cooldown;

        // Keyboard controls for pause/step.
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Space) {
                toggle_pause_requested = true;
            }
            if input.key_pressed(egui::Key::ArrowLeft) && can_manual_step {
                do_step_back = true;
            }
            if input.key_pressed(egui::Key::ArrowRight) && can_manual_step {
                do_step_forward = true;
            }
        });

        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let label = if self.paused { "Play" } else { "Pause" };
                if ui.button(label).clicked() {
                    toggle_pause_requested = true;
                }

                let back_label = if can_manual_step {
                    "Step back"
                } else {
                    "Step back (cooling)"
                };
                if ui
                    .add_enabled(self.current_index > 0 && can_manual_step, egui::Button::new(back_label))
                    .clicked()
                {
                    do_step_back = true;
                }
                let forward_label = if can_manual_step {
                    "Step forward"
                } else {
                    "Step forward (cooling)"
                };
                if ui
                    .add_enabled(can_manual_step, egui::Button::new(forward_label))
                    .clicked()
                {
                    do_step_forward = true;
                }
            });
        });

        if toggle_pause_requested {
            self.toggle_pause();
        }

        if do_step_back && self.current_index > 0 {
            self.current_index -= 1;
            self.restore_from_current();
            self.last_manual_step_instant = now_instant;
        }

        if do_step_forward {
            if !self.is_at_head() {
                self.current_index += 1;
                self.restore_from_current();
            } else {
                self.single_tick();
            }
            self.last_manual_step_instant = now_instant;
        }

        // Camera/viewport: first frame (or after reset) centers on the time crystal.
        // After that, a map click sets the focus (camera_focus_override); otherwise
        // the Time crystal tab centers on the crystal; building tabs on their building;
        // otherwise selected miner (if any); finally fall back to the time crystal.
        let view_width: usize = 40;
        let view_height: usize = 20;
        let (focus_x, focus_y) = if let Some((fx, fy)) = self.camera_focus_override {
            (fx, fy)
        } else if !self.camera_initialized {
            (self.world.spawn_x, self.world.spawn_y)
        } else if matches!(self.active_panel, ActivePanel::TimeCrystal) {
            (self.world.spawn_x, self.world.spawn_y)
        } else if matches!(self.active_panel, ActivePanel::Shop) {
            if let Some((sx, sy)) = self.shop_position {
                (sx, sy)
            } else {
                (self.world.spawn_x, self.world.spawn_y)
            }
        } else if matches!(self.active_panel, ActivePanel::Grindstone) {
            if let Some((gx, gy)) = self.grindstone_position {
                (gx, gy)
            } else {
                (self.world.spawn_x, self.world.spawn_y)
            }
        } else if matches!(self.active_panel, ActivePanel::TownHall) {
            if let Some((hx, hy)) = self.town_hall_position {
                (hx, hy)
            } else {
                (self.world.spawn_x, self.world.spawn_y)
            }
        } else if let Some(selected) = self.selected_miner {
            let idx = selected.min(self.miners.len().saturating_sub(1));
            let focus = &self.miners[idx];
            (focus.x, focus.y)
        } else {
            (self.world.spawn_x, self.world.spawn_y)
        };

        let half_w = view_width / 2;
        let half_h = view_height / 2;

        let mut view_origin_x = focus_x.saturating_sub(half_w);
        let mut view_origin_y = focus_y.saturating_sub(half_h);

        if self.world.width > view_width {
            let max_x = self.world.width - view_width;
            if view_origin_x > max_x {
                view_origin_x = max_x;
            }
        } else {
            view_origin_x = 0;
        }

        if self.world.height > view_height {
            let max_y = self.world.height - view_height;
            if view_origin_y > max_y {
                view_origin_y = max_y;
            }
        } else {
            view_origin_y = 0;
        }

        self.camera_x = view_origin_x;
        self.camera_y = view_origin_y;
        self.camera_w = view_width;
        self.camera_h = view_height;

        if !self.camera_initialized {
            self.camera_initialized = true;
        }

        let ascii = self.world.to_ascii(
            &self.miners,
            self.grindstone_position,
            self.shop_position,
            self.town_hall_position,
            self.harvested_ground,
            self.harvested_zero,
            self.harvested_q,
            self.harvested_d,
            self.harvested_g,
            self.harvested_c,
            self.step_counter,
            20,
            view_origin_x,
            view_origin_y,
            view_width,
            view_height,
        );

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Left: main ASCII map.
                ui.vertical(|ui| {
                    let job = self.build_colored_ascii(&ascii);
                    // Label that can be hovered and clicked.
                    let response = ui
                        .add(egui::Label::new(job).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);

                    // Handle clicks: select miners or focus buildings by clicking on them.
                    if let Some(click_pos) = response.interact_pointer_pos() {
                        if response.clicked() {
                            let rect = response.rect;
                            let local = click_pos - rect.min;

                            let view_width: usize = 40;
                            let view_height: usize = 20;
                            let char_w = rect.width() / view_width as f32;
                            let char_h = rect.height() / (view_height as f32 + 2.0);

                            if char_w > 0.0 && char_h > 0.0 {
                                let col = (local.x / char_w).floor() as isize;
                                let row = (local.y / char_h).floor() as isize;
                                if col >= 0
                                    && row >= 0
                                    && (col as usize) < view_width
                                    && (row as usize) < view_height
                                {
                                    let x = view_origin_x + col as usize;
                                    let y = view_origin_y + row as usize;

                                    // Any tile click: focus camera on this tile.
                                    self.camera_focus_override = Some((x, y));

                                    // Miner click: select miner and go to Miners tab.
                                    if let Some((idx, _)) = self
                                        .miners
                                        .iter()
                                        .enumerate()
                                        .find(|(_, m)| m.x == x && m.y == y)
                                    {
                                        self.selected_miner = Some(idx);
                                        self.active_panel = ActivePanel::Miners;
                                    // Building clicks: focus corresponding building tab.
                                    } else if self.grindstone_position == Some((x, y)) {
                                        self.selected_miner = None;
                                        self.active_panel = ActivePanel::Grindstone;
                                    } else if self.shop_position == Some((x, y)) {
                                        self.selected_miner = None;
                                        self.active_panel = ActivePanel::Shop;
                                    } else if self.town_hall_position == Some((x, y)) {
                                        self.selected_miner = None;
                                        self.active_panel = ActivePanel::TownHall;
                                    }
                                }
                            }
                        }
                    }

                    // Hover inspector for tiles/miners.
                    if let Some(hover_pos) = response.hover_pos() {
                        let rect = response.rect;
                        let local = hover_pos - rect.min;

                        // Approximate character cell size from the visible viewport area.
                        let view_width: usize = 40;
                        let view_height: usize = 20;
                        let char_w = rect.width() / view_width as f32;
                        let char_h =
                            rect.height() / (view_height as f32 + 2.0); // +2 for timer & counters

                        if char_w > 0.0 && char_h > 0.0 {
                            let col = (local.x / char_w).floor() as isize;
                            // Align hover more closely with the visible glyph row.
                            let row = (local.y / char_h).floor() as isize;

                            if col >= 0
                                && row >= 0
                                && (col as usize) < view_width
                                && (row as usize) < view_height
                            {
                                // Map from viewport coordinates back into world coordinates.
                                let x = view_origin_x + col as usize;
                                let y = view_origin_y + row as usize;

                                // Check for miner at this position.
                                let miner_at_pos = self
                                    .miners
                                    .iter()
                                    .find(|m| m.x == x && m.y == y);

                                let mut hover_text = String::new();

                                if let Some(miner) = miner_at_pos {
                                    use std::fmt::Write as _;
                                    let _ =
                                        writeln!(&mut hover_text, "Miner: {}", miner.name);
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "Type: {}",
                                        miner.miner_type
                                    );
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "HP: {}/{}",
                                        miner.hp, miner.max_hp
                                    );
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "Mana: {}/{}",
                                        miner.mana, miner.max_mana
                                    );
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "Movement: {}",
                                        miner.movement_type
                                    );
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "Color: {}",
                                        miner.color_name
                                    );
                                    let _ = writeln!(
                                        &mut hover_text,
                                        "Damage: {}",
                                        miner.pick_damage
                                    );
                                    if miner
                                        .inventory
                                        .iter()
                                        .any(|i| matches!(i, InventoryItem::Grindstone))
                                    {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Carrying: Grindstone"
                                        );
                                    }
                                    if miner
                                        .inventory
                                        .iter()
                                        .any(|i| matches!(i, InventoryItem::TownHall))
                                    {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Carrying: Town Hall"
                                        );
                                    }
                                } else {
                                    // Show tile info.
                                    use std::fmt::Write as _;
                                    if x == self.world.spawn_x && y == self.world.spawn_y
                                    {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Time crystal (T)"
                                        );
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "It feels familiar..."
                                        );
                                    } else if self.grindstone_position == Some((x, y))
                                    {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Grindstone (*)"
                                        );
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Sharpen your tools here."
                                        );
                                    } else if self.shop_position == Some((x, y)) {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Shop (S)"
                                        );
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "A small outpost for managing miners and upgrades."
                                        );
                                    } else if self.town_hall_position == Some((x, y))
                                    {
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "Town Hall (H)"
                                        );
                                        let _ = writeln!(
                                            &mut hover_text,
                                            "The administrative heart of your growing settlement."
                                        );
                                    } else {
                                        let cell =
                                            &self.world.tiles[self.world.idx(x, y)];
                                        match cell.tile {
                                            Tile::Ground => {
                                                let _ = writeln!(
                                                    &mut hover_text,
                                                    "Ground wall (#)"
                                                );
                                            }
                                            Tile::Empty => {
                                                let _ = writeln!(
                                                    &mut hover_text,
                                                    "Empty space"
                                                );
                                            }
                                            Tile::Path => {
                                                let _ = writeln!(
                                                    &mut hover_text,
                                                    "Path (.)"
                                                );
                                            }
                                            Tile::Ore(kind) => {
                                                let (ch, name) = match kind {
                                                    OreKind::Zero => {
                                                        ('0', "Zero ore")
                                                    }
                                                    OreKind::Q => ('Q', "Q ore"),
                                                    OreKind::D => ('D', "D ore"),
                                                    OreKind::G => ('G', "G ore"),
                                                    OreKind::C => ('C', "C ore"),
                                                };
                                                let _ = writeln!(
                                                    &mut hover_text,
                                                    "{} ({})",
                                                    name, ch
                                                );
                                            }
                                        }
                                        if cell.max_durability > 0 {
                                            let _ = writeln!(
                                                &mut hover_text,
                                                "Durability: {}/{}",
                                                cell.durability,
                                                cell.max_durability
                                            );
                                        }
                                    }
                                }

                                if !hover_text.is_empty() {
                                    let popup_pos =
                                        hover_pos + egui::vec2(12.0, 12.0);
                                    egui::Area::new(egui::Id::new("hover_popup"))
                                        .fixed_pos(popup_pos)
                                        .show(ctx, |ui| {
                                            ui.set_min_width(220.0);
                                            egui::Frame::popup(&ctx.style())
                                                .fill(egui::Color32::from_rgb(
                                                    20, 20, 20,
                                                ))
                                                .stroke(egui::Stroke::new(
                                                    1.0,
                                                    egui::Color32::WHITE,
                                                ))
                                                .rounding(egui::Rounding::same(4.0))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        hover_text.trim_end(),
                                                    );
                                                });
                                        });
                                }
                            }
                        }
                    }
                });

                // Right: simple event log.
                ui.vertical(|ui| {
                    ui.heading("Events");
                    let log_height = ui.available_height().min(200.0);
                    egui::ScrollArea::vertical()
                        .max_height(log_height)
                        .show(ui, |ui| {
                            for line in self.event_log.iter().rev() {
                                ui.label(line);
                            }
                        });
                });
            });

            ui.separator();

            // Panel selector: Miners, Time crystal, Shop.
            ui.horizontal(|ui| {
                let miners_clicked = ui
                    .selectable_label(
                        matches!(
                            self.active_panel,
                            ActivePanel::Miners
                                | ActivePanel::Grindstone
                                | ActivePanel::TownHall
                        ),
                        "Miners",
                    )
                    .clicked();
                let time_crystal_clicked = ui
                    .selectable_label(
                        matches!(self.active_panel, ActivePanel::TimeCrystal),
                        "Time crystal",
                    )
                    .clicked();

                let mut shop_clicked = false;
                if self.shop_position.is_some() {
                    shop_clicked = ui
                        .selectable_label(
                            matches!(self.active_panel, ActivePanel::Shop),
                            "Shop",
                        )
                        .clicked();
                }

                if miners_clicked {
                    self.active_panel = ActivePanel::Miners;
                } else if time_crystal_clicked {
                    self.active_panel = ActivePanel::TimeCrystal;
                } else if shop_clicked {
                    self.active_panel = ActivePanel::Shop;
                }
            });

            ui.separator();

            match self.active_panel {
                ActivePanel::Shop => {
                    ui.heading("Shop");

                    // Grindstone purchase and placement (unlocks the Grindstone tab once placed).
                    let grind_cost_ground: u32 = 10;
                    let grind_cost_zero: u32 = 1;
                    if self.grindstone_position.is_none() {
                        ui.separator();
                        ui.label("Buildings:");
                        if self.harvested_ground >= grind_cost_ground
                            && self.harvested_zero >= grind_cost_zero
                        {
                            if ui
                                .button(format!(
                                    "Buy Grindstone ({} #, {} 0) – miners will place it",
                                    grind_cost_ground, grind_cost_zero
                                ))
                                .clicked()
                            {
                                self.harvested_ground -= grind_cost_ground;
                                self.harvested_zero -= grind_cost_zero;
                                self.pending_grindstones = self
                                    .pending_grindstones
                                    .saturating_add(1);
                            }
                        } else {
                            ui.label(format!(
                                "Need {} # and {} 0 to buy a Grindstone.",
                                grind_cost_ground, grind_cost_zero
                            ));
                        }
                    } else {
                        ui.label("Grindstone: placed in the world.");
                    }

                    // Town Hall purchase and placement (another building, placed by miners from the time crystal).
                    let town_hall_cost_ground: u32 = 20;
                    if self.town_hall_position.is_none() {
                        if self.harvested_ground >= town_hall_cost_ground {
                            if ui
                                .button(format!(
                                    "Build Town Hall ({} #) – miners will place it",
                                    town_hall_cost_ground
                                ))
                                .clicked()
                            {
                                self.harvested_ground -= town_hall_cost_ground;
                                self.pending_town_halls = self
                                    .pending_town_halls
                                    .saturating_add(1);
                            }
                        } else {
                            ui.label(format!(
                                "Need {} # to build a Town Hall.",
                                town_hall_cost_ground
                            ));
                        }
                    } else {
                        ui.label("Town Hall: established.");
                    }

                    // Miner purchase (capped at 8).
                    ui.separator();
                    ui.label("Miners:");
                    if self.miners.len() >= 8 {
                        ui.separator();
                        ui.label("You already have the maximum of 8 miners.");
                    } else if self.harvested_ground >= 10 && self.harvested_zero >= 1 {
                        if ui.button("Add miner (10 #, 1 0)").clicked() {
                            self.harvested_ground -= 10;
                            self.harvested_zero -= 1;

                            // Spawn new miner in an empty space around the time crystal.
                            let spawn_x = self.world.spawn_x;
                            let spawn_y = self.world.spawn_y;
                            let mut x = spawn_x;
                            let mut y = spawn_y;
                            'outer_spawn: for dy in -1isize..=1 {
                                for dx in -1isize..=1 {
                                    if dx == 0 && dy == 0 {
                                        continue;
                                    }
                                    let nx = spawn_x as isize + dx;
                                    let ny = spawn_y as isize + dy;
                                    if nx >= 0
                                        && nx < self.world.width as isize
                                        && ny >= 0
                                        && ny < self.world.height as isize
                                    {
                                        let idx = self.world.idx(nx as usize, ny as usize);
                                        if matches!(
                                            self.world.tiles[idx].tile,
                                            Tile::Empty
                                        ) && !self
                                            .miners
                                            .iter()
                                            .any(|m| m.home_x == nx as usize
                                                && m.home_y == ny as usize)
                                        {
                                            x = nx as usize;
                                            y = ny as usize;
                                            break 'outer_spawn;
                                        }
                                    }
                                }
                            }

                            let mut rng = rand::thread_rng();
                            let name = self.generate_miner_name(&mut rng);
                            // Random elemental type.
                            const TYPES: &[&str] =
                                &["Water", "Wind", "Fire", "Lightning", "Earth"];
                            let miner_type = TYPES
                                .choose(&mut rng)
                                .unwrap_or(&"Earth")
                                .to_string();

                            // Random HP/mana in [5, 10].
                            let hp_dist = Uniform::new_inclusive(5u32, 10u32);
                            let hp = rng.sample(hp_dist);
                            let mana = rng.sample(hp_dist);

                            let miner_index = self.miners.len();
                            let (color_name, color) =
                                App::miner_color_for_index(miner_index);

                            self.miners.push(Miner {
                                x,
                                y,
                                home_x: x,
                                home_y: y,
                                name,
                                miner_type,
                                hp,
                                max_hp: hp,
                                mana,
                                max_mana: mana,
                                movement_distance: 1,
                                movement_type: "Random".to_string(),
                                color_name,
                                color,
                                target: None,
                                pick_damage: 1,
                                task: None,
                                path: Vec::new(),
                                path_index: 0,
                                inventory: Vec::new(),
                                pathing_style: self.pathing_style,
                            });
                        }
                    } else {
                        ui.label("Need 10 # and 1 0 to add a miner.");
                    }
                }
                ActivePanel::TimeCrystal => {
                    ui.heading("Time crystal");
                    ui.label("The crystal at the centre of the cavern marks where each cycle begins. Miners awaken here.");
                    ui.separator();
                    ui.label("It feels familiar...");

                    ui.separator();
                    ui.heading("Structures");
                    let shop_cost_ground: u32 = 5;
                    // Only allow buying a shop if none is placed; multiple orders queue until placed.
                    if self.shop_position.is_none() {
                        if self.harvested_ground >= shop_cost_ground {
                            if ui
                                .button(format!(
                                    "Buy Shop ({} #) – unlocks building a shop",
                                    shop_cost_ground
                                ))
                                .clicked()
                            {
                                self.harvested_ground -= shop_cost_ground;
                                self.pending_shops = self.pending_shops.saturating_add(1);
                            }
                        } else {
                            ui.label(format!(
                                "Need {} # to buy a Shop.",
                                shop_cost_ground
                            ));
                        }
                    } else {
                        ui.label("Shop: placed in the world. Switch to the Shop tab to view it.");
                    }
                }
                ActivePanel::Miners | ActivePanel::TownHall | ActivePanel::Grindstone => {
                    // Miner selection and full detail panel.
                    if self.miners.is_empty() {
                        ui.label("No miners yet.");
                    } else {
                        let max_index = self.miners.len().saturating_sub(1);
                        if self.selected_miner.is_none() {
                            self.selected_miner = Some(0);
                        }
                        let sel = self.selected_miner.unwrap_or(0);
                        let clamped = sel.min(max_index);
                        self.selected_miner = Some(clamped);

                        egui::ComboBox::from_label("Select miner")
                            .selected_text(&self.miners[clamped].name)
                            .show_ui(ui, |ui| {
                                for (i, miner) in self.miners.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.selected_miner,
                                        Some(i),
                                        &miner.name,
                                    );
                                }
                            });

                        let miner = &mut self.miners[clamped];

                        // 1. Miner stats
                        ui.separator();
                        ui.collapsing("Miner stats", |ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut miner.name);
                            ui.label(format!("Type: {}", miner.miner_type));
                            ui.label(format!("HP: {}/{}", miner.hp, miner.max_hp));
                            ui.label(format!("Mana: {}/{}", miner.mana, miner.max_mana));
                            ui.label(format!(
                                "Movement distance: {}",
                                miner.movement_distance
                            ));
                            ui.label(format!("Damage: {}", miner.pick_damage));
                        });

                        // 2. Miner inventory
                        ui.separator();
                        ui.collapsing("Miner inventory", |ui| {
                            if miner.inventory.is_empty() {
                                ui.label("Empty");
                            } else {
                                for item in &miner.inventory {
                                    ui.label(item.to_string());
                                }
                            }
                        });

                        // 3. Miner upgrades (grindstone sharpening) – only once a grindstone exists.
                        if self.grindstone_position.is_some() {
                            ui.separator();
                            ui.collapsing("Miner upgrades", |ui| {
                                let max_damage: u8 = 5;
                                if miner.pick_damage >= max_damage {
                                    ui.label(
                                        "This miner's tools are as sharp as they can get.",
                                    );
                                } else {
                                    let next_damage = miner.pick_damage + 1;
                                    let cost_ground: u32 = 1;

                                    ui.label(format!(
                                        "Upgrade pickaxe to damage {} (cost: {} #)",
                                        next_damage, cost_ground
                                    ));
                                    if self.harvested_ground < cost_ground {
                                        ui.label("Not enough # to sharpen.");
                                    } else if ui.button("Sharpen pickaxe").clicked() {
                                        self.harvested_ground = self
                                            .harvested_ground
                                            .saturating_sub(cost_ground);
                                        miner.task = Some(MinerTask::GoToGrindstone);
                                        miner.path.clear();
                                        miner.path_index = 0;
                                    }
                                }
                            });
                        }

                        // 4. Designation (appearance + AI), only after the Town Hall exists.
                        if self.town_hall_position.is_some() {
                            ui.separator();
                            ui.collapsing("Designation", |ui| {
                                // Appearance
                                ui.label("Appearance:");
                            let color_options: &[(&str, egui::Color32)] = &[
                                ("White", egui::Color32::WHITE),
                                ("Green", egui::Color32::GREEN),
                                ("Blue", egui::Color32::BLUE),
                                ("Red", egui::Color32::RED),
                                ("Yellow", egui::Color32::YELLOW),
                                ("Cyan", egui::Color32::from_rgb(0, 255, 255)),
                                ("Magenta", egui::Color32::from_rgb(255, 0, 255)),
                            ];

                                let current_color_name = miner.color_name.clone();
                                let mut new_color: Option<(String, egui::Color32)> = None;

                                egui::ComboBox::from_label("Color")
                                    .selected_text(&current_color_name)
                                    .show_ui(ui, |ui| {
                                        for (name, color) in color_options {
                                            let selected = current_color_name == *name;
                                            if ui
                                                .selectable_label(selected, *name)
                                                .clicked()
                                            {
                                                new_color =
                                                    Some(((*name).to_string(), *color));
                                            }
                                        }
                                    });

                                if let Some((name, color)) = new_color {
                                    miner.color_name = name;
                                    miner.color = color;
                                }

                                // AI / pathing
                                ui.separator();
                                ui.label("Assign AI (pathing style):");
                                let style_name = match miner.pathing_style {
                                    PathingStyle::Random => "Random wandering",
                                    PathingStyle::ClosestOreFromMiner => "Nearest wall",
                                    PathingStyle::ClosestOreFromSpawn => {
                                        "Closest ore (from time crystal)"
                                    }
                                };
                                ui.label(format!("Current: {}", style_name));

                                if ui
                                    .selectable_label(
                                        matches!(
                                            miner.pathing_style,
                                            PathingStyle::Random
                                        ),
                                        "Random wandering",
                                    )
                                    .clicked()
                                {
                                    miner.pathing_style = PathingStyle::Random;
                                }
                                if ui
                                    .selectable_label(
                                        matches!(
                                            miner.pathing_style,
                                            PathingStyle::ClosestOreFromMiner
                                        ),
                                        "Nearest wall",
                                    )
                                    .clicked()
                                {
                                    miner.pathing_style =
                                        PathingStyle::ClosestOreFromMiner;
                                }
                                if ui
                                    .selectable_label(
                                        matches!(
                                            miner.pathing_style,
                                            PathingStyle::ClosestOreFromSpawn
                                        ),
                                        "Closest ore (from time crystal)",
                                    )
                                    .clicked()
                                {
                                    miner.pathing_style =
                                        PathingStyle::ClosestOreFromSpawn;
                                }
                            });
                        }
                    }
                }
            }
        });

        // Keep animating even if nothing changes so ticks run and NPC moves.
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ASCII Idle Game",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

