# pfinder

A production-quality, retro 3D first-person shooter that runs entirely inside your Linux terminal. Built with Rust.

```
         _______
  _______|      >        ╔─────────────────────────────────────────╗
 |_____________|         ║ HP ████████████ 100 │ AMMO 30/30        ║
                         ║ [W/S] Move · [A/D] Turn · [SPC] Shoot  ║
                         ╚─────────────────────────────────────────╝
```

---

## Overview

pfinder uses the classic **Raycasting** algorithm (à la Wolfenstein 3D) to render a procedurally generated 2D grid maze into a pseudo-3D viewport, drawn entirely with colored Unicode block characters and ANSI escape codes — no graphics library, no OpenGL, just your terminal.

Every run generates a brand-new randomized maze. Find the **yellow exit portal** before the 90-second timer runs out. Enemies patrol the corridors and will chase you on sight. Stay alive, conserve ammo, and escape.

---

## Features

- **Raycasting engine** — DDA algorithm with fisheye correction and perpendicular wall distance projection
- **Double-buffered rendering** — differential cell updates at a locked 60 FPS target; no flicker
- **Procedural maze generation** — Depth-First Search (recursive backtracking) on a 12×12 room grid, fully connected on every run
- **Unicode block shading** — depth-based wall shading (`█ ▓ ▒ ░`) with lit/shadowed face differentiation
- **Enemy AI** — patrol and line-of-sight chase states with collision-aware movement
- **Hitscan shooting** — instant center-ray hit detection against enemy bounding cylinders
- **Probability loot drops** — 25% health pack, 12.5% ammo pack on enemy death
- **Retro audio** — procedurally generated sine and square waves via rodio (shoot, damage, pickup, victory, game over)
- **Screen effects** — damage flash border, camera shake, muzzle flash on the gun sprite
- **Crosshair + gun sprite** — always-visible first-person weapon with animated fire state

---

## Requirements

- **Rust** 1.70+ — [rustup.rs](https://rustup.rs)
- **Linux** with a terminal emulator that supports:
  - 256-color ANSI (`$TERM=xterm-256color` or equivalent)
  - UTF-8 / Unicode block characters
  - Minimum terminal size: **80 × 24**
- **ALSA** development libraries (for audio):
  ```bash
  # Debian / Ubuntu
  sudo apt install libasound2-dev

  # Arch Linux
  sudo pacman -S alsa-lib

  # Fedora
  sudo dnf install alsa-lib-devel
  ```
  > Audio is **optional** — if no audio device is found, the game runs silently.

**Recommended terminals:** kitty, Alacritty, foot, WezTerm, GNOME Terminal.  
Keyboard enhancement flags (for smooth held-key movement) are enabled automatically on supported terminals.

---

## Installation

```bash
git clone <repo-url>
cd pfinder
cargo run --release
```

The `--release` flag is strongly recommended — debug builds may drop below 60 FPS on slower machines.

---

## Controls

| Key | Action |
|---|---|
| `W` / `↑` | Move forward |
| `S` / `↓` | Move backward |
| `A` | Turn left |
| `D` | Turn right |
| `←` | Strafe left |
| `→` | Strafe right |
| `Space` | Shoot |
| `R` | Restart (new map) |
| `Q` / `Esc` | Quit |

---

## Gameplay

### Objective
Navigate the randomized maze and reach the **yellow exit portal** hidden in the far corner before the **90-second timer** expires.

### Enemies
- Enemies (`M`) **patrol** randomly until they spot you
- Line-of-sight detection triggers **chase mode** — they run toward you and deal melee damage on contact
- Kill an enemy with `Space` (hitscan — instant hit along the crosshair center ray)
- Dead enemies (`X`) remain as corpses and may drop loot

### Loot Drops
| Item | Chance | Effect |
|---|---|---|
| Health Pack `+` | 25% (1-in-4) | +20 HP (capped at 100) |
| Ammo Pack `*` | 12.5% (1-in-8) | +10 ammo (capped at 30) |

Walk over loot to pick it up automatically.

### HUD
```
╔────────────────────────────────────────────────────────────╗
║ HP ████████████ 100 │ AMMO 30/30 │ TIME  74s │ N │ FIND THE EXIT ║
║ [W/S] Move · [A/D] Turn · [← →] Strafe · [SPACE] Shoot · [R] Restart · [Q/ESC] Quit ║
╚────────────────────────────────────────────────────────────╝
```

- **HP bar** — green above 60%, yellow above 25%, red below
- **AMMO** — cyan when plentiful, yellow when low, red when critical
- **TIME** — white above 30s, yellow above 10s, red below
- **Compass** — cardinal direction you are currently facing (no minimap by design)

---

## Visual System

### Wall shading by distance

| Distance | Glyph | Meaning |
|---|---|---|
| < 4 | `█` | Very close — full block |
| 4 – 7 | `▓` | Near — dark block |
| 7 – 11 | `▒` | Medium — medium shade |
| 11 – 15 | `░` | Far — light shade |
| > 15 | ` ` | Fog — invisible |

East/West-facing walls are rendered brighter than North/South-facing walls, providing natural depth shading without textures.

### Aspect ratio correction
Terminal character cells are approximately **2× taller than wide** (8 px × 16 px in most fonts). The engine applies a `0.5` vertical scale factor to wall heights so the projected geometry looks proportionally correct instead of squished.

---

## Architecture

```
src/
├── main.rs         Entry point
├── engine.rs       Game loop, input, state, HUD, double-buffer flush
├── map.rs          Procedural maze generation (DFS)
├── raycaster.rs    DDA raycasting, sprite projection, buffer, screen renders
└── audio.rs        PCM tone synthesis, rodio integration
```

### Key design decisions

**Double buffering** — `Buffer` holds the full frame as a `Vec<Vec<Cell>>`. Each frame, only cells that changed from the previous frame are written to stdout (cursor repositioning + color codes). This eliminates flicker without clearing the screen.

**Input handling** — A timer-decay system bridges the gap between 60 FPS game logic and the OS keyboard repeat rate (~30 Hz). Each key press refreshes a per-key frame counter; movement continues smoothly while the counter is live and stops within ~50 ms of key release. On terminals with keyboard enhancement flags (kitty protocol), Release events provide instant stop.

**Sprite projection** — Enemies and loot are transformed into camera space with the inverse camera matrix, then projected to screen columns. A Z-buffer (per-column wall distances) occludes sprites behind walls correctly.

---

## Dependencies

| Crate | Purpose |
|---|---|
| [`crossterm`](https://crates.io/crates/crossterm) 0.27 | Raw terminal mode, alternate screen, ANSI colors, non-blocking input |
| [`rand`](https://crates.io/crates/rand) 0.8 | Maze generation, enemy patrol angles, loot probability |
| [`rodio`](https://crates.io/crates/rodio) 0.19 | Audio output; custom `ToneSource` generates sine/square PCM waves |

---

## License

MIT
