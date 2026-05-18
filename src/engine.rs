use std::io::{self, Write};
use std::time::{Duration, Instant};
use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal,
};
use rand::Rng;
use crate::audio::Audio;
use crate::map::Map;
use crate::raycaster::{self, Buffer, Cell};

const TIMER_START: f64     = 90.0;
const MOVE_SPEED: f64      = 3.0;
const ROT_SPEED: f64       = 1.8;
const PLAYER_MAX_HP: i32   = 100;
const PLAYER_MAX_AMMO: i32 = 30;
const HUD_ROWS: usize      = 4;   // separator + status + keys + separator
const FOV_PLANE: f64       = 0.80; // wider FOV — less tunnel-vision
const TARGET_FRAME: Duration = Duration::from_micros(16_667);

// How many frames a key stays "active" after the last event.
// At 60 FPS and ~30 Hz keyboard repeat, 20 frames = 333 ms covers the
// 250 ms initial repeat delay and avoids stuck-key without Release events.
const HOLD_FRAMES: u64 = 20;

#[derive(PartialEq, Clone, Copy)]
enum Phase { Playing, GameOver, Victory }

// ── Player ────────────────────────────────────────────────────────────────────
struct Player {
    x: f64, y: f64,
    dir_x: f64, dir_y: f64,
    plane_x: f64, plane_y: f64,
    hp: i32, ammo: i32,
}
impl Player {
    fn new(x: f64, y: f64) -> Self {
        Player { x, y, dir_x: 1.0, dir_y: 0.0,
                 plane_x: 0.0, plane_y: FOV_PLANE,
                 hp: PLAYER_MAX_HP, ammo: PLAYER_MAX_AMMO }
    }
    fn rotate(&mut self, a: f64) {
        let (c, s) = (a.cos(), a.sin());
        let (dx, dy, px, py) = (self.dir_x, self.dir_y, self.plane_x, self.plane_y);
        self.dir_x   = dx * c - dy * s;
        self.dir_y   = dx * s + dy * c;
        self.plane_x = px * c - py * s;
        self.plane_y = px * s + py * c;
    }
    fn try_move(&mut self, map: &Map, dx: f64, dy: f64) {
        let nx = self.x + dx;
        let ny = self.y + dy;
        if !map.is_wall(nx as i32, self.y as i32) { self.x = nx; }
        if !map.is_wall(self.x as i32, ny as i32) { self.y = ny; }
    }
    fn compass(&self) -> &'static str {
        let d = (self.dir_y.atan2(self.dir_x).to_degrees() + 360.0) as u32 % 360;
        match d { 315..=360 | 0..=44 => "E", 45..=134 => "S", 135..=224 => "W", _ => "N" }
    }
}

// ── Enemy ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq)]
enum EnemyState { Patrol, Chase, Dead }
struct Enemy {
    x: f64, y: f64, hp: i32,
    state: EnemyState,
    patrol_angle: f64, patrol_timer: f64,
}
impl Enemy {
    fn new(x: f64, y: f64) -> Self {
        Enemy { x, y, hp: 2, state: EnemyState::Patrol,
                patrol_angle: 0.0, patrol_timer: 0.0 }
    }
}

// ── Loot ──────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy)]
struct Loot { x: f64, y: f64, kind: u8 }

fn has_los(map: &Map, ax: f64, ay: f64, bx: f64, by: f64) -> bool {
    let (dx, dy) = (bx - ax, by - ay);
    let d = (dx * dx + dy * dy).sqrt();
    if d < 0.001 { return true; }
    let n = (d / 0.2) as usize + 1;
    (1..n).all(|i| {
        let t = i as f64 / n as f64;
        !map.is_wall((ax + dx * t) as i32, (ay + dy * t) as i32)
    })
}

// ── Key state (timer-decay, no stuck keys) ────────────────────────────────────
// Indices: 0=fwd 1=back 2=turn_left 3=turn_right 4=strafe_l 5=strafe_r
struct Keys {
    timers: [u64; 6],   // last frame# this key had an event
    frame:  u64,
}
impl Keys {
    fn new() -> Self { Keys { timers: [0; 6], frame: 0 } }

    fn tick(&mut self) { self.frame += 1; }

    // Call on Press OR Repeat events
    fn press(&mut self, idx: usize) {
        self.timers[idx] = self.frame;
    }
    // Call on Release events (terminal with enhancement flags)
    fn release(&mut self, idx: usize) {
        self.timers[idx] = 0;
    }
    // Is this key currently considered held?
    fn held(&self, idx: usize) -> bool {
        self.timers[idx] > 0
            && self.frame.saturating_sub(self.timers[idx]) < HOLD_FRAMES
    }
}

// ── Public entry point ────────────────────────────────────────────────────────
pub fn run() -> io::Result<()> {
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::DisableLineWrap,
    )?;
    // Keyboard enhancement for Release events (silently ignored if unsupported)
    let _ = execute!(out, PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            | KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
    ));

    let result = game_loop(&mut out);

    let _ = execute!(out, PopKeyboardEnhancementFlags);
    execute!(out,
        ResetColor, cursor::Show,
        terminal::LeaveAlternateScreen,
        terminal::EnableLineWrap,
    )?;
    terminal::disable_raw_mode()?;
    result
}

// ── Main game loop ────────────────────────────────────────────────────────────
fn game_loop(out: &mut impl Write) -> io::Result<()> {
    let mut rng   = rand::thread_rng();
    let audio: Option<Audio> = std::panic::catch_unwind(Audio::new).unwrap_or(None);
    let mut keys;

    'outer: loop {
        let map     = Map::generate(&mut rng);
        let mut player  = Player::new(map.spawn.0, map.spawn.1);
        let mut enemies: Vec<Enemy> = map.enemy_spawns.iter()
            .map(|&(x,y)| Enemy::new(x,y)).collect();
        let mut loot: Vec<Loot> = Vec::new();
        let mut time_left     = TIMER_START;
        let mut phase         = Phase::Playing;
        let mut flash_timer   = 0.0f64;
        let mut shake_timer   = 0.0f64;
        let mut gun_flash     = 0.0f64; // muzzle-flash timer
        let mut prev_buf: Option<Buffer> = None;
        let mut last_frame  = Instant::now();

        execute!(out, terminal::Clear(terminal::ClearType::All))?;
        keys = Keys::new(); // reset held state on every new game

        'game: loop {
            keys.tick();

            let now = Instant::now();
            let dt  = now.duration_since(last_frame).as_secs_f64().min(0.05);
            last_frame = now;

            let (cols, rows) = terminal::size()?;
            let vw      = cols as usize;
            let total_h = rows as usize;
            let vh      = total_h.saturating_sub(HUD_ROWS);

            // ── Single-pass event processing ──────────────────────────────
            let mut shoot   = false;
            let mut quit    = false;
            let mut restart = false;

            while event::poll(Duration::ZERO)? {
                match event::read()? {
                    Event::Key(KeyEvent { code, kind, modifiers, .. }) => {
                        let press   = matches!(kind,
                            KeyEventKind::Press | KeyEventKind::Repeat);
                        let release = kind == KeyEventKind::Release;

                        match code {
                            KeyCode::Char('w') | KeyCode::Up => {
                                if press   { keys.press(0); }
                                if release { keys.release(0); }
                            }
                            KeyCode::Char('s') | KeyCode::Down => {
                                if press   { keys.press(1); }
                                if release { keys.release(1); }
                            }
                            KeyCode::Char('a') => {
                                if press   { keys.press(2); }
                                if release { keys.release(2); }
                            }
                            KeyCode::Char('d') => {
                                if press   { keys.press(3); }
                                if release { keys.release(3); }
                            }
                            KeyCode::Left => {
                                if press   { keys.press(4); }
                                if release { keys.release(4); }
                            }
                            KeyCode::Right => {
                                if press   { keys.press(5); }
                                if release { keys.release(5); }
                            }
                            KeyCode::Char(' ') if press         => shoot   = true,
                            KeyCode::Char('r') if press         => restart = true,
                            KeyCode::Esc | KeyCode::Char('q') if press => quit = true,
                            KeyCode::Char('c')
                                if press
                                    && modifiers.contains(KeyModifiers::CONTROL) => {
                                quit = true;
                            }
                            _ => {}
                        }
                    }
                    Event::Resize(_, _) => {
                        prev_buf = None;
                        execute!(out, terminal::Clear(terminal::ClearType::All))?;
                    }
                    _ => {}
                }
            }

            if quit                              { break 'outer; }
            if restart && phase != Phase::Playing { break 'game;  }

            // ── Game logic ────────────────────────────────────────────────
            if phase == Phase::Playing {
                let ms = MOVE_SPEED * dt;
                let rs = ROT_SPEED  * dt;

                if keys.held(2) { player.rotate( rs); }
                if keys.held(3) { player.rotate(-rs); }
                if keys.held(0) {
                    let (dx, dy) = (player.dir_x * ms, player.dir_y * ms);
                    player.try_move(&map, dx, dy);
                }
                if keys.held(1) {
                    let (dx, dy) = (-player.dir_x * ms, -player.dir_y * ms);
                    player.try_move(&map, dx, dy);
                }
                if keys.held(4) {
                    let (dx, dy) = (-player.plane_x * ms, -player.plane_y * ms);
                    player.try_move(&map, dx, dy);
                }
                if keys.held(5) {
                    let (dx, dy) = (player.plane_x * ms, player.plane_y * ms);
                    player.try_move(&map, dx, dy);
                }

                // Hitscan shoot
                if shoot && player.ammo > 0 {
                    player.ammo -= 1;
                    gun_flash = 0.10; // 100 ms muzzle flash
                    if let Some(ref a) = audio { a.shoot(); }
                    let ray = raycaster::cast_ray(
                        &map, player.x, player.y,
                        player.dir_x, player.dir_y,
                    );
                    for e in enemies.iter_mut() {
                        if e.state == EnemyState::Dead { continue; }
                        let (ex, ey) = (e.x - player.x, e.y - player.y);
                        let d = (ex * ex + ey * ey).sqrt();
                        if d >= ray.dist { continue; }
                        let dot = ex / d * player.dir_x + ey / d * player.dir_y;
                        if dot > 0.92 {
                            e.hp -= 1;
                            if e.hp <= 0 {
                                e.state = EnemyState::Dead;
                                if rng.gen_bool(0.25) {
                                    loot.push(Loot { x: e.x, y: e.y, kind: 0 });
                                } else if rng.gen_bool(0.125) {
                                    loot.push(Loot { x: e.x, y: e.y, kind: 1 });
                                }
                            }
                        }
                    }
                }

                // Timer
                time_left -= dt;
                if time_left <= 0.0 {
                    time_left = 0.0;
                    phase = Phase::GameOver;
                    if let Some(ref a) = audio { a.gameover(); }
                }

                // Victory
                if map.is_exit(player.x as i32, player.y as i32) {
                    phase = Phase::Victory;
                    if let Some(ref a) = audio { a.victory(); }
                }

                // Enemy AI
                let mut hit = false;
                for i in 0..enemies.len() {
                    if enemies[i].state == EnemyState::Dead { continue; }
                    let (edx, edy) = (player.x - enemies[i].x, player.y - enemies[i].y);
                    let dist = (edx * edx + edy * edy).sqrt();
                    let sees = dist < 12.0
                        && has_los(&map, enemies[i].x, enemies[i].y, player.x, player.y);

                    match enemies[i].state {
                        EnemyState::Patrol => {
                            if sees {
                                enemies[i].state = EnemyState::Chase;
                                if let Some(ref a) = audio { a.enemy_alert(); }
                            }
                            enemies[i].patrol_timer -= dt;
                            if enemies[i].patrol_timer <= 0.0 {
                                enemies[i].patrol_angle =
                                    rng.gen_range(0.0..std::f64::consts::TAU);
                                enemies[i].patrol_timer = rng.gen_range(0.8..2.2);
                            }
                            let sp  = 0.8 * dt;
                            let nx  = enemies[i].x + enemies[i].patrol_angle.cos() * sp;
                            let ny  = enemies[i].y + enemies[i].patrol_angle.sin() * sp;
                            if !map.is_wall(nx as i32, enemies[i].y as i32) {
                                enemies[i].x = nx;
                            } else {
                                enemies[i].patrol_timer = 0.0;
                            }
                            if !map.is_wall(enemies[i].x as i32, ny as i32) {
                                enemies[i].y = ny;
                            }
                        }
                        EnemyState::Chase => {
                            if !sees && dist > 14.0 {
                                enemies[i].state = EnemyState::Patrol;
                            }
                            if dist > 0.001 {
                                let sp = 1.0 * dt;
                                let nx = enemies[i].x + (edx / dist) * sp;
                                if !map.is_wall(nx as i32, enemies[i].y as i32) {
                                    enemies[i].x = nx;
                                }
                                let ny = enemies[i].y + (edy / dist) * sp;
                                if !map.is_wall(enemies[i].x as i32, ny as i32) {
                                    enemies[i].y = ny;
                                }
                            }
                            if dist < 0.9 {
                                hit = true;
                                // knock enemy back away from player
                                let (kick_dx, kick_dy) = (-edx / dist, -edy / dist);
                                for _ in 0..20 {
                                    let nx = enemies[i].x + kick_dx * 0.2;
                                    let ny = enemies[i].y + kick_dy * 0.2;
                                    if !map.is_wall(nx as i32, enemies[i].y as i32) { enemies[i].x = nx; }
                                    if !map.is_wall(enemies[i].x as i32, ny as i32) { enemies[i].y = ny; }
                                }
                                // briefly patrol away before re-engaging
                                enemies[i].patrol_angle = kick_dy.atan2(kick_dx);
                                enemies[i].patrol_timer = 1.2;
                                enemies[i].state = EnemyState::Patrol;
                            }
                        }
                        EnemyState::Dead => {}
                    }
                }
                if hit {
                    player.hp -= 10;
                    flash_timer = 0.25;
                    shake_timer = 0.15;
                    if let Some(ref a) = audio { a.damage(); }
                    if player.hp <= 0 {
                        player.hp = 0;
                        phase = Phase::GameOver;
                        if let Some(ref a) = audio { a.gameover(); }
                    }
                }

                // Loot pickup
                loot.retain(|item| {
                    let (dx, dy) = (item.x - player.x, item.y - player.y);
                    if dx * dx + dy * dy < 0.36 {
                        match item.kind {
                            0 => player.hp   = (player.hp   + 20).min(PLAYER_MAX_HP),
                            _ => player.ammo = (player.ammo + 10).min(PLAYER_MAX_AMMO),
                        }
                        if let Some(ref a) = audio { a.pickup(); }
                        false
                    } else { true }
                });

                flash_timer = (flash_timer - dt).max(0.0);
                shake_timer = (shake_timer - dt).max(0.0);
                gun_flash   = (gun_flash   - dt).max(0.0);
            }

            // ── Render ────────────────────────────────────────────────────
            let mut buf = Buffer::new(vw, total_h);
            match phase {
                Phase::Playing => {
                    let flash = flash_timer > 0.0;
                    let (sx, sy) = if shake_timer > 0.0 {
                        (rng.gen_range(-1i32..=1), rng.gen_range(-1i32..=1))
                    } else { (0, 0) };
                    let esprites: Vec<(f64, f64, bool)> = enemies.iter()
                        .map(|e| (e.x, e.y, e.state != EnemyState::Dead)).collect();
                    let lsprites: Vec<(f64, f64, u8)> = loot.iter()
                        .map(|l| (l.x, l.y, l.kind)).collect();
                    raycaster::render_view(
                        &map, player.x, player.y,
                        player.dir_x, player.dir_y,
                        player.plane_x, player.plane_y,
                        &esprites, &lsprites,
                        &mut buf, vh, flash, sx, sy,
                        gun_flash > 0.0,
                    );
                    let exit_world = (map.exit.0 as f64 + 0.5, map.exit.1 as f64 + 0.5);
                    render_hud(&mut buf, &player, time_left, vw, vh, exit_world);
                }
                Phase::GameOver => raycaster::render_gameover(&mut buf),
                Phase::Victory  => raycaster::render_victory(&mut buf, time_left, player.ammo),
            }

            flush_buffer(out, &buf, &mut prev_buf)?;

            let elapsed = Instant::now().duration_since(last_frame);
            if elapsed < TARGET_FRAME {
                std::thread::sleep(TARGET_FRAME - elapsed);
            }
        }
    }
    Ok(())
}

// ── HUD ───────────────────────────────────────────────────────────────────────
// Layout (4 rows):
//  r0: ╔══ top border ══════════════════════════════════════════════════════╗
//  r1: ║  HP bar │ AMMO │ TIME │ compass │ goal                            ║
//  r2: ║  key reference                                                    ║
//  r3: ╚══ bottom border ═══════════════════════════════════════════════════╝
fn exit_arrow(player: &Player, exit: (f64, f64)) -> (&'static str, u32) {
    let dx = exit.0 - player.x;
    let dy = exit.1 - player.y;
    let dist = ((dx * dx + dy * dy).sqrt()) as u32;
    let fwd  = dx * player.dir_x + dy * player.dir_y;
    let side = dy * player.dir_x - dx * player.dir_y;
    let deg  = side.atan2(fwd).to_degrees();
    let arrow = if      deg >= -22.5 && deg <  22.5 { "↑" }
                else if deg >=  22.5 && deg <  67.5 { "↗" }
                else if deg >=  67.5 && deg < 112.5 { "→" }
                else if deg >= 112.5 && deg < 157.5 { "↘" }
                else if deg >=  157.5 || deg < -157.5 { "↓" }
                else if deg >= -157.5 && deg < -112.5 { "↙" }
                else if deg >= -112.5 && deg <  -67.5 { "←" }
                else                                   { "↖" };
    (arrow, dist)
}

fn render_hud(buf: &mut Buffer, player: &Player, time_left: f64, vw: usize, vh: usize, exit: (f64, f64)) {
    let (r0, r1, r2, r3) = (vh, vh + 1, vh + 2, vh + 3);
    if r3 >= buf.height { return; }

    let sep_color = Color::AnsiValue(240);
    let bg        = Color::Black;

    // Borders
    for x in 0..vw {
        buf.put(x, r0, '─', sep_color, bg);
        buf.put(x, r3, '─', sep_color, bg);
        buf.put(x, r1, ' ', Color::White, bg);
        buf.put(x, r2, ' ', Color::White, bg);
    }
    buf.put(0,    r0, '╔', sep_color, bg); buf.put(vw-1, r0, '╗', sep_color, bg);
    buf.put(0,    r3, '╚', sep_color, bg); buf.put(vw-1, r3, '╝', sep_color, bg);
    buf.put(0,    r1, '║', sep_color, bg); buf.put(vw-1, r1, '║', sep_color, bg);
    buf.put(0,    r2, '║', sep_color, bg); buf.put(vw-1, r2, '║', sep_color, bg);

    // ── Status row (r1) ──────────────────────────────────────────────────────
    let mut cx = 2usize;

    macro_rules! write_hud {
        ($row:expr, $s:expr, $fg:expr) => {{
            for ch in $s.chars() {
                if cx + 1 >= vw { break; }
                buf.put(cx, $row, ch, $fg, bg);
                cx += 1;
            }
        }};
    }
    macro_rules! sep_char {
        ($row:expr) => {{
            write_hud!($row, " │ ", sep_color);
        }};
    }

    // HP bar
    let hp_pct   = player.hp.max(0) as f64 / PLAYER_MAX_HP as f64;
    let bar_fill = (hp_pct * 12.0).round() as usize;
    let hp_color = if player.hp > 60 { Color::AnsiValue(40) }
                   else if player.hp > 25 { Color::AnsiValue(220) }
                   else { Color::AnsiValue(196) };

    write_hud!(r1, "HP ", Color::AnsiValue(250));
    for i in 0..12 {
        if cx + 1 >= vw { break; }
        let (ch, fg) = if i < bar_fill {
            ('█', hp_color)
        } else {
            ('░', Color::AnsiValue(236))
        };
        buf.put(cx, r1, ch, fg, bg); cx += 1;
    }
    write_hud!(r1, &format!(" {}", player.hp), Color::AnsiValue(250));

    sep_char!(r1);

    // Ammo
    let ammo_color = if player.ammo > 10 { Color::AnsiValue(87) }
                     else if player.ammo > 0 { Color::AnsiValue(220) }
                     else { Color::AnsiValue(196) };
    write_hud!(r1, "AMMO ", Color::AnsiValue(250));
    write_hud!(r1, &format!("{:2}/{}", player.ammo, PLAYER_MAX_AMMO), ammo_color);

    sep_char!(r1);

    // Timer
    let time_color = if time_left > 30.0 { Color::AnsiValue(250) }
                     else if time_left > 10.0 { Color::AnsiValue(220) }
                     else { Color::AnsiValue(196) };
    write_hud!(r1, "TIME ", Color::AnsiValue(250));
    write_hud!(r1, &format!("{:3.0}s", time_left), time_color);

    sep_char!(r1);

    // Exit direction compass
    let (arrow, edist) = exit_arrow(player, exit);
    write_hud!(r1, &format!("EXIT {} {}u", arrow, edist), Color::AnsiValue(46));

    // ── Key reference row (r2) ───────────────────────────────────────────────
    cx = 2;

    let key_bg    = Color::AnsiValue(235);
    let label_fg  = Color::AnsiValue(244);

    macro_rules! keybind {
        ($keys:expr, $desc:expr) => {{
            // Key label with subtle highlight
            for ch in $keys.chars() {
                if cx + 1 >= vw { break; }
                buf.put(cx, r2, ch, Color::AnsiValue(226), key_bg);
                cx += 1;
            }
            buf.put(cx, r2, ' ', label_fg, bg); cx += 1;
            for ch in $desc.chars() {
                if cx + 1 >= vw { break; }
                buf.put(cx, r2, ch, label_fg, bg);
                cx += 1;
            }
            // spacer between entries
            if cx + 3 < vw {
                buf.put(cx, r2, ' ', label_fg, bg); cx += 1;
                buf.put(cx, r2, '·', Color::AnsiValue(238), bg); cx += 1;
                buf.put(cx, r2, ' ', label_fg, bg);
                cx = cx.saturating_add(1);
            }
        }};
    }

    keybind!("[W/S]",    "Move");
    keybind!("[A/D]",    "Turn");
    keybind!("[← →]",   "Strafe");
    keybind!("[SPACE]",  "Shoot");
    keybind!("[R]",      "Restart");
    keybind!("[Q/ESC]",  "Quit");
    let _ = cx; // consumed after last keybind spacer
}

// ── Double-buffer differential flush ─────────────────────────────────────────
fn flush_buffer(out: &mut impl Write, curr: &Buffer, prev: &mut Option<Buffer>) -> io::Result<()> {
    let (w, h)  = (curr.width, curr.height);
    let mut buf: Vec<u8> = Vec::with_capacity(w * h * 10);
    let mut last_fg  = Color::Reset;
    let mut last_bg  = Color::Reset;
    let mut last_pos: Option<(usize, usize)> = None;

    let blank = Buffer::new(0, 0);
    let prev_ref = prev.as_ref().unwrap_or(&blank);

    for y in 0..h {
        for x in 0..w {
            let cell = curr.cells[y][x];
            let old  = if y < prev_ref.height && x < prev_ref.width {
                prev_ref.cells[y][x]
            } else {
                Cell { ch: '\0', fg: Color::Reset, bg: Color::Reset }
            };
            if cell == old { continue; }

            let sequential = last_pos.map_or(false, |(py, px)| py == y && px == x);
            if !sequential {
                write!(buf, "\x1B[{};{}H", y + 1, x + 1)?;
            }
            if cell.fg != last_fg {
                write!(buf, "{}", SetForegroundColor(cell.fg))?;
                last_fg = cell.fg;
            }
            if cell.bg != last_bg {
                write!(buf, "{}", SetBackgroundColor(cell.bg))?;
                last_bg = cell.bg;
            }
            write!(buf, "{}", cell.ch)?;
            last_pos = Some((y, x + 1));
        }
    }

    out.write_all(&buf)?;
    out.flush()?;
    *prev = Some(Buffer { cells: curr.cells.clone(), width: curr.width, height: curr.height });
    Ok(())
}
