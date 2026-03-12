use eframe::egui;
use game_core::{self, GameState};
use rand::Rng;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy)]
enum Tile {
    Ground,
    Empty,
}

struct World {
    width: usize,
    height: usize,
    tiles: Vec<Tile>,
}

impl World {
    fn new(width: usize, height: usize) -> Self {
        let mut tiles = vec![Tile::Ground; width * height];

        // Create a walkable gap along the bottom row.
        let bottom_y = height.saturating_sub(1);
        let gap_start = width / 4;
        let gap_end = width / 4 * 3;
        for x in gap_start..gap_end {
            let idx = bottom_y * width + x;
            tiles[idx] = Tile::Empty;
        }

        Self {
            width,
            height,
            tiles,
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    fn bottom_y(&self) -> usize {
        self.height.saturating_sub(1)
    }

    /// Convert world + NPC + inventory into ASCII.
    fn to_ascii(&self, npc_x: usize, npc_y: usize, inventory: u32) -> String {
        let mut s = String::with_capacity(self.width * (self.height + 4));

        for y in 0..self.height {
            for x in 0..self.width {
                let ch = if x == npc_x && y == npc_y {
                    '@'
                } else {
                    match self.tiles[self.idx(x, y)] {
                        Tile::Ground => '#',
                        Tile::Empty => ' ',
                    }
                };
                s.push(ch);
            }
            s.push('\n');
        }

        // Inventory counter under the map, e.g. "#: 12".
        use std::fmt::Write as _;
        let _ = write!(&mut s, "#: {}\n", inventory);

        s
    }
}

struct App {
    world: World,
    game: GameState,
    last_tick_instant: Instant,
    last_save_instant: Instant,
    npc_x: usize,
    npc_y: usize,
    harvested: u32,
}

impl App {
    fn new() -> Self {
        let world_width = 40;
        let world_height = 20;
        let world = World::new(world_width, world_height);

        let game = game_core::load_or_new();

        let bottom_y = world_height.saturating_sub(1);
        let npc_x = world_width / 2;
        let npc_y = bottom_y;

        let now_instant = Instant::now();

        Self {
            world,
            game,
            last_tick_instant: now_instant,
            last_save_instant: now_instant,
            npc_x,
            npc_y,
            harvested: 0,
        }
    }

    fn tick_game_if_needed(&mut self) {
        let now_instant = Instant::now();
        let since_last = now_instant.duration_since(self.last_tick_instant);

        // Run the core tick and NPC step at ~2.5 Hz to slow movement.
        if since_last >= Duration::from_millis(400) {
            let now = SystemTime::now();
            self.game.tick(now);
            self.step_npc_random();
            self.last_tick_instant = now_instant;
        }
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
    /// If it steps onto a `#`, the tile is cleared and converted into
    /// one inventory `#` in the bottom row.
    fn step_npc_random(&mut self) {
        let mut rng = rand::thread_rng();
        let dir = rng.gen_range(0..4);
        let (dx, dy) = match dir {
            0 => (-1isize, 0isize), // left
            1 => (1, 0),            // right
            2 => (0, -1),           // up
            _ => (0, 1),            // down
        };

        let mut new_x = self.npc_x as isize + dx;
        let mut new_y = self.npc_y as isize + dy;

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
            return;
        }

        let nx = new_x as usize;
        let ny = new_y as usize;
        let idx = self.world.idx(nx, ny);

        // If we step onto a ground tile, harvest it.
        if let Tile::Ground = self.world.tiles[idx] {
            self.world.tiles[idx] = Tile::Empty;
            self.harvested = self.harvested.saturating_add(1);
        }

        self.npc_x = nx;
        self.npc_y = ny;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick_game_if_needed();
        self.autosave_if_needed();

        let ascii = self.world.to_ascii(self.npc_x, self.npc_y, self.harvested);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(egui::RichText::new(ascii).monospace());
        });

        // Keep animating even if nothing changes so ticks run and NPC moves.
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "ASCII Idle Game",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

