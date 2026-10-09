# lazy-mouse

A system-wide "lazy brush" for the Windows mouse cursor, inspired by the
[lazy-brush](https://github.com/dulnan/lazy-brush) library. The cursor only
follows your hand once you move outside a small circle, and it is pulled along
like something dragged on a rubber band. A dashed rope on screen shows the
connection between the cursor and where your hand really is.

Useful for smoothing out shaky drawing, tracing, or just for fun.

## How it works

- Your real mouse position is tracked as the **pointer** (the red dot). The
  cursor you see is the **brush** (the rope's other end).
- While the pointer is inside the lazy circle (`radius` px around the brush),
  the brush does not move.
- When the pointer leaves the circle, it pulls the brush like a rubber band:
  the pull grows faster than linearly the further it is stretched, so the
  brush accelerates as the band tightens.
- When the pointer comes back inside the circle, the brush **stops instantly**.
- On a mouse button press, the pointer snaps to the brush, so every stroke
  starts at the cursor with the full lazy area around it.

Technically: a low-level mouse hook (`WH_MOUSE_LL`) swallows real mouse moves
and moves the cursor to the brush position with `SetCursorPos`. A click-through
layered window draws the rope.

## Requirements

- Windows (the program prints a message and exits on other platforms)
- [Rust](https://www.rust-lang.org/tools/install) (2024 edition toolchain)

## Build and run

```
cargo run --release
```

or with settings:

```
cargo run --release -- 60 0 250 18 80
```

The program keeps a console window open. Closing it or pressing the quit
hotkey exits the program.

## Hotkeys

| Keys | Action |
|---|---|
| `Ctrl+Alt+L` | Toggle the effect on or off |
| `Ctrl+Alt+Q` | Quit |

The effect starts **off**; press `Ctrl+Alt+L` to enable it.

## Settings

Settings can be given as positional command-line arguments, in this order:

```
lazy-mouse [radius] [friction] [stiffness] [damping] [stretch]
```

Missing or invalid values fall back to the default. Out-of-range values are
clamped.

| Setting | Short | Default | Range | Meaning |
|---|---|---|---|---|
| `radius` | `r` | 40 | 0 .. 300 | Size of the lazy area in px |
| `friction` | `f` | 0 | 0 .. 1 | Weakens the pull; `1` means the brush never moves |
| `stiffness` | `s` | 180 | 1 .. 2000 | Higher = snappier pull |
| `damping` | `d` | 22 | 0 .. 300 | Lower = the brush builds speed more freely |
| `stretch` | `x` | 100 | 1 .. 1000000 | Lower = more nonlinear (stiffens faster when stretched); very large = plain linear spring |

### Changing settings while running

Type commands into the console. Several can be combined on one line:

```
r 60 s 250 d 18
friction 0.3 stretch 50
quit
```

Full names (`radius`, `friction`, `stiffness`, `damping`, `stretch`, `quit`) and
short names (`r`, `f`, `s`, `d`, `x`, `q`) both work. Changes apply
immediately. Words that are not understood are reported as `ignored: ...`.

### Tuning tips

- **Snappier or looser:** raise or lower `stiffness`.
- **More rubbery acceleration:** lower `damping`.
- **More nonlinear pull:** lower `stretch` (try 30 to 50).
- **Heavier feel:** raise `friction`.

## Debug environment variables

| Variable | Effect |
|---|---|
| `LAZY_DEBUG=1` | Log button events (brush and pointer positions) to stderr |
| `LAZY_NO_OVERLAY=1` | Do not create the rope window, to tell overlay problems from hook problems |

PowerShell example:

```
$env:LAZY_DEBUG = 1; cargo run --release
```

## Project layout

```
src/
  main.rs     entry point
  config.rs   parameter table, limits, CLI and console parsing
  brush.rs    rubber-band lazy-brush math
  rope.rs     catenary rope between two points
  render.rs   tiny-skia drawing of the rope and pointer dot
  win.rs      mouse hook, overlay window, message loop (Windows only)
```

`brush.rs`, `config.rs`, `rope.rs` and `render.rs` contain no OS calls, so their
unit tests run on any platform:

```
cargo test
```

## Known limitations

- The effect applies to the whole desktop, including every monitor.
- Windows may restrict input hooks and cursor movement over windows running
  with higher privileges (for example an elevated app). If the effect does not
  work there, try running lazy-mouse as administrator.
- Other programs that move the cursor (injected input) are not smoothed.