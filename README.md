# lazy-mouse

A lazy-brush mouse smoother for Windows. The cursor only moves when your real
pointer leaves a circle of a chosen radius around it, then trails behind like a
rope. Handy for drawing smooth lines in any app (Photoshop, Krita, Blender, ...).

A thin overlay draws the rope from the cursor to a red dot showing where your
real pointer is.

> Windows only. On other systems the program just prints a message and exits.

## Download

Every commit is built automatically. Grab the latest `.exe` from the
**Releases** page of this repository. No installer, just run it.

## Usage

Run it from a terminal (keep the window open, it also takes commands):

```
lazy-mouse [radius_px] [friction]
```

| Argument    | Default | Meaning                                                                     |
| ----------- | ------- | --------------------------------------------------------------------------- |
| `radius_px` | `40`    | Size of the lazy area in pixels. `0` = cursor follows exactly.              |
| `friction`  | `0`     | `0`..`1`. Higher = the cursor catches up more slowly. `1` = it never moves. |

Examples:

```
lazy-mouse              # radius 40, no friction
lazy-mouse 80 0.3       # longer rope, a bit of drag
```

### Hotkeys

| Keys         | Action                       |
| ------------ | ---------------------------- |
| `Ctrl+Alt+L` | Turn the lazy mouse on / off |
| `Ctrl+Alt+Q` | Quit                         |

It starts **off**. Press `Ctrl+Alt+L` to enable it.

### Changing settings while it runs

Type into the same console window and press Enter:

```
radius 60
friction 0.3
r 80 f 0.5      # short forms, several values at once
quit
```

If it is currently off, the new values are used the next time you turn it on.
The radius is capped at 300 px.

### Environment variables

| Variable            | Effect                                                                    |
| ------------------- | ------------------------------------------------------------------------- |
| `LAZY_DEBUG=1`      | Log mouse button events to stderr.                                        |
| `LAZY_NO_OVERLAY=1` | Don't draw the rope window (to tell hook problems from overlay problems). |

PowerShell example: `$env:LAZY_DEBUG = 1; .\lazy-mouse.exe`

## Build from source

Requirements:

- Windows 10 or 11
- [Rust](https://rustup.rs) (stable) with the MSVC toolchain
  (Visual Studio Build Tools, "Desktop development with C++")

```
git clone <your-repo-url>
cd lazy-mouse
cargo run --release -- 60 0.2     # build and run with radius 60, friction 0.2
cargo build --release             # exe ends up in target\release\
cargo test                        # runs the brush and renderer tests
```

Optional: smaller and faster binary. Add to `Cargo.toml`:

```toml
[profile.release]
lto = true
codegen-units = 1
strip = true
```

## Automatic releases

`.github/workflows/release.yml` runs on every push to `main` or `master`:

1. runs the tests,
2. builds the release exe on a Windows runner,
3. publishes a GitHub release tagged `build-<run number>` with the exe attached
   and auto-generated notes.

You can also start it by hand from the **Actions** tab (_Run workflow_).

If the release step fails with a permissions error, open
**Settings > Actions > General > Workflow permissions** and choose
_Read and write permissions_.

## Troubleshooting

- **Cursor feels stuck:** friction is too high. Try `friction 0`.
- **Windows SmartScreen or antivirus warns about the exe:** it installs a
  global mouse hook and the exe is unsigned, which triggers heuristics. Build it
  yourself if you prefer.
- **No effect over some windows:** if a program runs as administrator, try
  starting lazy-mouse as administrator too.
- **Stuck cursor after a crash:** press `Ctrl+Alt+Q`, or end the process in
  Task Manager. The hook is removed when the process exits.

## Project layout

| File            | Purpose                                              |
| --------------- | ---------------------------------------------------- |
| `src/main.rs`   | Argument parsing, entry point                        |
| `src/brush.rs`  | Lazy-brush math (pure, tested)                       |
| `src/rope.rs`   | Rope curve sampling                                  |
| `src/render.rs` | Software renderer for the overlay (tiny-skia)        |
| `src/win.rs`    | Windows mouse hook, overlay window, hotkeys, console |
