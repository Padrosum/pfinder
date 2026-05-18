use crossterm::style::Color;
use crate::map::Map;

// ── Buffer ────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}
impl Cell {
    pub const EMPTY: Cell = Cell { ch: ' ', fg: Color::Black, bg: Color::Black };
}

pub struct Buffer {
    pub cells: Vec<Vec<Cell>>,
    pub width: usize,
    pub height: usize,
}
impl Buffer {
    pub fn new(w: usize, h: usize) -> Self {
        Buffer {
            cells: vec![vec![Cell::EMPTY; w.max(1)]; h.max(1)],
            width: w,
            height: h,
        }
    }
    #[inline]
    pub fn put(&mut self, x: usize, y: usize, ch: char, fg: Color, bg: Color) {
        if x < self.width && y < self.height {
            self.cells[y][x] = Cell { ch, fg, bg };
        }
    }
    #[inline]
    pub fn put_str(&mut self, x: usize, y: usize, s: &str, fg: Color, bg: Color) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i, y, ch, fg, bg);
        }
    }
}

// ── Ray cast (DDA) ────────────────────────────────────────────────────────────
pub struct RayHit {
    pub dist: f64,
    pub side: u8,
    #[allow(dead_code)] pub map_x: i32,
    #[allow(dead_code)] pub map_y: i32,
    pub is_exit: bool,
}

pub fn cast_ray(map: &Map, px: f64, py: f64, rdx: f64, rdy: f64) -> RayHit {
    let mut mx = px as i32;
    let mut my = py as i32;
    let ddx = if rdx == 0.0 { f64::INFINITY } else { rdx.abs().recip() };
    let ddy = if rdy == 0.0 { f64::INFINITY } else { rdy.abs().recip() };
    let (step_x, mut sdx) = if rdx < 0.0 { (-1i32, (px - mx as f64) * ddx) }
                            else           { ( 1i32, (mx as f64 + 1.0 - px) * ddx) };
    let (step_y, mut sdy) = if rdy < 0.0 { (-1i32, (py - my as f64) * ddy) }
                            else           { ( 1i32, (my as f64 + 1.0 - py) * ddy) };
    loop {
        let (side, dist) = if sdx < sdy {
            sdx += ddx; mx += step_x; (0u8, sdx - ddx)
        } else {
            sdy += ddy; my += step_y; (1u8, sdy - ddy)
        };
        let c = map.cell(mx, my);
        if c == 1 || c == 3 {
            return RayHit { dist, side, map_x: mx, map_y: my, is_exit: c == 3 };
        }
    }
}

// ── Wall appearance ───────────────────────────────────────────────────────────
// Terminal chars are ~2× taller than wide (8px × 16px).
// The 0.5 multiplier corrects wall height so proportions look natural.
const WALL_SCALE: f64 = 0.5;
// Sprites are a bit taller so they stay visible at range.
const SPRITE_SCALE: f64 = 0.65;

fn wall_glyph(dist: f64, side: u8) -> (char, Color) {
    // side 0 = E/W face (lit), side 1 = N/S face (shadowed)
    // 8 bands instead of 4 — smoother distance transitions, less eye strain
    let (ch, lit, dim): (char, u8, u8) =
        if      dist < 2.0  { ('█', 254, 250) }
        else if dist < 4.0  { ('█', 251, 246) }
        else if dist < 6.0  { ('▓', 248, 243) }
        else if dist < 8.0  { ('▓', 245, 240) }
        else if dist < 10.0 { ('▒', 242, 237) }
        else if dist < 12.0 { ('▒', 239, 234) }
        else if dist < 15.0 { ('░', 236, 232) }
        else if dist < 19.0 { ('░', 233, 232) }
        else { return (' ', Color::Black) };
    let idx = if side == 0 { lit } else { dim };
    (ch, Color::AnsiValue(idx))
}

fn exit_glyph(dist: f64) -> (char, Color) {
    // Bright yellow portal
    match dist as u32 {
        0..=3 => ('█', Color::AnsiValue(226)),
        4..=7 => ('▓', Color::AnsiValue(220)),
        8..=12=> ('▒', Color::AnsiValue(214)),
        _     => ('░', Color::AnsiValue(136)),
    }
}

// ── Gun sprite constants ──────────────────────────────────────────────────────
const GUN_IDLE: [&str; 3] = [
    "         _______ ",
    "  _______|      >",
    " |_____________| ",
];
const GUN_FIRE: [&str; 3] = [
    "         _______  ",
    "  _______|    *>>>",
    " |_____________|  ",
];

// ── Main render ───────────────────────────────────────────────────────────────
pub fn render_view(
    map: &Map,
    px: f64, py: f64,
    dir_x: f64, dir_y: f64,
    plane_x: f64, plane_y: f64,
    enemies: &[(f64, f64, bool)],
    loot_items: &[(f64, f64, u8)],
    buf: &mut Buffer,
    view_h: usize,
    damage_flash: bool,
    shake_x: i32,
    shake_y: i32,
    gun_flash: bool,
) {
    let vw   = buf.width;
    let vh   = view_h;
    let half = (vh / 2) as i32;

    let mut z_buf = vec![f64::INFINITY; vw];

    // ── Ceiling and floor ─────────────────────────────────────────────────────
    for y in 0..vh {
        let bg = if y < vh / 2 {
            let t = 1.0 - y as f64 / (vh / 2) as f64;
            let v = (232.0 + t * 7.0).min(239.0) as u8; // 232..239
            Color::AnsiValue(v)
        } else {
            let t = (y - vh / 2) as f64 / (vh / 2) as f64;
            let v = (239u8).saturating_sub((t * 7.0) as u8); // 239..232
            Color::AnsiValue(v)
        };
        for x in 0..vw {
            buf.put(x, y, ' ', Color::Black, bg);
        }
    }

    // ── Raycasting ────────────────────────────────────────────────────────────
    for col in 0..vw {
        let cam_x = 2.0 * col as f64 / vw as f64 - 1.0;
        let rdx   = dir_x + plane_x * cam_x;
        let rdy   = dir_y + plane_y * cam_x;

        let hit  = cast_ray(map, px, py, rdx, rdy);
        let dist = hit.dist.max(0.001);
        z_buf[col] = dist;

        // WALL_SCALE = 0.5 corrects for terminal character aspect ratio (2:1 h/w)
        let line_h = ((vh as f64 / dist) * WALL_SCALE) as i32;
        let draw_s = (half - line_h / 2).max(0) as usize;
        let draw_e = (half + line_h / 2).min(vh as i32 - 1) as usize;

        let (ch, mut fg) = if hit.is_exit { exit_glyph(dist) }
                           else            { wall_glyph(dist, hit.side) };

        if damage_flash { fg = Color::Red; }
        let bg = if damage_flash { Color::AnsiValue(52) } else { Color::Black };

        let scol = ((col as i32 + shake_x).max(0) as usize).min(vw - 1);
        for row in draw_s..=draw_e {
            let srow = ((row as i32 + shake_y).max(0) as usize).min(vh - 1);
            buf.put(scol, srow, ch, fg, bg);
        }
    }

    // ── Sprite rendering ──────────────────────────────────────────────────────
    // Tuple: (world_x, world_y, face_char, fg_color, bg_color)
    let mut sprites: Vec<(f64, f64, char, Color, Color)> = Vec::new();

    for &(lx, ly, kind) in loot_items {
        if kind == 0 {
            sprites.push((lx, ly, '+', Color::White, Color::AnsiValue(22))); // dark green
        } else {
            sprites.push((lx, ly, '*', Color::Black, Color::AnsiValue(220))); // yellow
        }
    }
    for &(ex, ey, alive) in enemies {
        if alive {
            sprites.push((ex, ey, 'M', Color::White, Color::AnsiValue(124))); // bold red
        } else {
            sprites.push((ex, ey, 'X', Color::AnsiValue(240), Color::Black));
        }
    }

    // Sort farthest-first (painter's algorithm)
    sprites.sort_by(|a, b| {
        let da = (a.0 - px).powi(2) + (a.1 - py).powi(2);
        let db = (b.0 - px).powi(2) + (b.1 - py).powi(2);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });

    let inv_det = 1.0 / (plane_x * dir_y - dir_x * plane_y);

    for (sx, sy, sch, sfg, sbg) in sprites {
        let (dx, dy) = (sx - px, sy - py);
        let tx = inv_det * ( dir_y * dx - dir_x * dy);
        let ty = inv_det * (-plane_y * dx + plane_x * dy);
        if ty <= 0.05 { continue; }

        let screen_x = ((vw as f64 / 2.0) * (1.0 + tx / ty)) as i32;

        // Slightly larger than walls (SPRITE_SCALE > WALL_SCALE) for visibility
        let sprite_h = ((vh as f64 / ty) * SPRITE_SCALE) as i32;
        // Width compensates for char aspect ratio: ~half as wide as tall in pixels
        let sprite_w = ((sprite_h / 2) + 1).max(2);

        let row0 = (half - sprite_h / 2).max(0);
        let row1 = (half + sprite_h / 2).min(vh as i32 - 1);
        let col0 = (screen_x - sprite_w / 2).max(0);
        let col1 = (screen_x + sprite_w / 2).min(vw as i32 - 1);

        for col in col0..=col1 {
            if col < 0 || col >= vw as i32 { continue; }
            if ty >= z_buf[col as usize] { continue; } // behind wall
            let draw_ch = if col == screen_x { sch } else { '█' };
            for row in row0..=row1 {
                if row < 0 || row >= vh as i32 { continue; }
                buf.put(col as usize, row as usize, draw_ch, sfg, sbg);
            }
        }
    }

    // ── Crosshair ─────────────────────────────────────────────────────────────
    {
        let cx = vw / 2;
        let cy = vh / 2;
        buf.put(cx, cy, '·', Color::AnsiValue(255), Color::Black);
    }

    // ── Gun sprite ────────────────────────────────────────────────────────────
    {
        let lines = if gun_flash { &GUN_FIRE } else { &GUN_IDLE };
        let gun_w  = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let gun_x  = vw.saturating_sub(gun_w + 1);
        let gun_y0 = vh.saturating_sub(3);

        for (i, line) in lines.iter().enumerate() {
            let y = gun_y0 + i;
            if y >= vh { break; }
            for (j, ch) in line.chars().enumerate() {
                let x = gun_x + j;
                if x >= vw || ch == ' ' { continue; }
                let (fg, bg) = if gun_flash && matches!(ch, '*' | '>') {
                    (Color::Black, Color::AnsiValue(226))   // bright yellow flash
                } else {
                    (Color::AnsiValue(248), Color::Black)   // steel gray gun
                };
                buf.put(x, y, ch, fg, bg);
            }
        }
    }

    // ── Damage flash border ───────────────────────────────────────────────────
    if damage_flash {
        for y in 0..vh {
            buf.put(0,      y, '▌', Color::AnsiValue(196), Color::AnsiValue(52));
            buf.put(vw - 1, y, '▐', Color::AnsiValue(196), Color::AnsiValue(52));
        }
        for x in 0..vw {
            buf.put(x, 0,      '▄', Color::AnsiValue(196), Color::AnsiValue(52));
            buf.put(x, vh - 1, '▀', Color::AnsiValue(196), Color::AnsiValue(52));
        }
    }
}

// ── Game Over ─────────────────────────────────────────────────────────────────
pub fn render_gameover(buf: &mut Buffer) {
    let (w, h) = (buf.width, buf.height);
    for y in 0..h {
        for x in 0..w {
            buf.put(x, y, if (x + y) % 2 == 0 { '▒' } else { ' ' },
                    Color::AnsiValue(124), Color::Black);
        }
    }
    let art: &[(&str, Color)] = &[
        ("  ██████   █████  ███    ███ ███████", Color::AnsiValue(196)),
        (" ██       ██   ██ ████  ████ ██     ", Color::AnsiValue(203)),
        (" ██   ███ ███████ ██ ████ ██ █████  ", Color::AnsiValue(203)),
        (" ██    ██ ██   ██ ██  ██  ██ ██     ", Color::AnsiValue(203)),
        ("  ██████  ██   ██ ██      ██ ███████", Color::AnsiValue(196)),
        ("", Color::Black),
        ("  ██████  ██    ██ ███████ ██████   ", Color::AnsiValue(196)),
        (" ██    ██ ██    ██ ██      ██   ██  ", Color::AnsiValue(203)),
        (" ██    ██ ██    ██ █████   ██████   ", Color::AnsiValue(203)),
        (" ██    ██  ██  ██  ██      ██   ██  ", Color::AnsiValue(203)),
        ("  ██████    ████   ███████ ██   ██  ", Color::AnsiValue(196)),
        ("", Color::Black),
        ("  You died or ran out of time.", Color::AnsiValue(244)),
        ("", Color::Black),
        ("  [ R ] New Game     [ Q ] Quit", Color::White),
    ];
    let start_y = h.saturating_sub(art.len() + 2) / 2;
    for (i, &(line, fg)) in art.iter().enumerate() {
        let sx = w.saturating_sub(line.len()) / 2;
        buf.put_str(sx, start_y + i, line, fg, Color::Black);
    }
}

// ── Victory ───────────────────────────────────────────────────────────────────
pub fn render_victory(buf: &mut Buffer, time_left: f64, ammo: i32) {
    let (w, h) = (buf.width, buf.height);
    for y in 0..h {
        for x in 0..w {
            buf.put(x, y, if (x + y) % 2 == 0 { '▒' } else { ' ' },
                    Color::AnsiValue(28), Color::Black);
        }
    }
    let stats = format!("  Time remaining: {:.0}s   Ammo left: {}", time_left, ammo);
    let art: &[(&str, Color)] = &[
        (" ██    ██ ██  ██████ ████████  ██████  ██████  ██    ██", Color::AnsiValue(46)),
        ("  ██  ██  ██ ██         ██    ██    ██ ██   ██  ██  ██ ", Color::AnsiValue(83)),
        ("   ████   ██ ██         ██    ██    ██ ██████    ████  ", Color::AnsiValue(83)),
        ("    ██    ██ ██         ██    ██    ██ ██   ██    ██   ", Color::AnsiValue(83)),
        ("    ██    ██  ██████    ██     ██████  ██   ██    ██   ", Color::AnsiValue(46)),
        ("", Color::Black),
        ("  You escaped the maze!", Color::White),
        ("", Color::Black),
        (&stats, Color::AnsiValue(220)),
        ("", Color::Black),
        ("  [ R ] New Game     [ Q ] Quit", Color::White),
    ];
    let start_y = h.saturating_sub(art.len() + 2) / 2;
    for (i, &(line, fg)) in art.iter().enumerate() {
        let sx = w.saturating_sub(line.len()) / 2;
        buf.put_str(sx, start_y + i, line, fg, Color::Black);
    }
}
