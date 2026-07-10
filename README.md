# Atlas

A spatial Wayland compositor built with [Smithay](https://github.com/Smithay/smithay).

Atlas treats your desktop as an infinite canvas: windows live in a shared
coordinate space that you can pan and zoom, rather than being confined to
physical monitor boundaries.  It is **pre-alpha** — the architecture is
being proven out crate by crate.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   compositor (bin)                   │
│  loads atlas.kdl, calls atlas_core::winit::run()    │
├─────────────────────────────────────────────────────┤
│                    atlas-core                        │
│  winit backend, event loop, render pipeline,         │
│  state machine, Smithay handler impls                │
├────────┬──────┬──────┬──────┬──────┬──────┬─────────┤
│atlas-  │atlas- │atlas-│atlas-│atlas-│atlas-│atlas-   │
│space   │config │render│ input│output│ layout│  wm     │
│(Global │ (KDL  │(Gles │(evdev│(output│(floating│(window │
│Space + │parser)│pipeline)│seat) │mgt)  │tiling) │rules)  │
│Viewport)│      │      │      │      │       │        │
├────────┴──────┴──────┴──────┴──────┴──────┴─────────┤
│          atlas-plugin-api / atlas-plugin              │
│  WASM plugin runtime (wasmtime) + WIT interface       │
│                    atlas-ipc                          │
│  Unix-socket IPC for external tools                   │
└──────────────────────────────────────────────────────┘
```

### Current active crates

| Crate | Status | Role |
|-------|--------|------|
| `compositor` | ✅ Live | Binary entry point |
| `atlas-core` | ✅ Live | Winit backend, state machine, Smithay handlers, render loop |
| `atlas-space` | ✅ Live | `GlobalSpace` — infinite-canvas coordinate manager + `Viewport` |
| `atlas-config` | ✅ Live | KDL config parser (knuffel derive) |

### Stub crates (planned, not yet implemented)

| Crate | Purpose |
|-------|---------|
| `atlas-render` | Custom render pipeline (border-radius shader, damage tracking) |
| `atlas-input` | Input event routing (keybind engine, pointer constraints, touch) |
| `atlas-output` | Multi-monitor output management (`wlr-output-management`) |
| `atlas-layout` | Layout engines: floating, tiling, snap clusters |
| `atlas-wm` | Window management, rules, workspaces |
| `atlas-animation` | Spring physics + easing animation system |
| `atlas-plugin-api` | Shared WIT types for WASM plugins |
| `atlas-plugin` | WASM runtime (wasmtime) to load external plugins |
| `atlas-ipc` | JSON-over-Unix-socket IPC protocol |

## Build & Run

```bash
# Build the compositor
cargo build -p compositor

# Run (development)
RUST_LOG=info cargo run -p compositor
```

**Dependencies:** Rust 2021 edition, the standard Smithay build dependencies
(libwayland, libxkbcommon, libgl, libseat, udev).  On a typical Debian/Ubuntu
system:

```bash
sudo apt install build-essential pkg-config libwayland-dev libxkbcommon-dev \
  libegl1-mesa-dev libgles2-mesa-dev libseat-dev libudev-dev
```

### Creating `atlas.kdl`

The compositor reads `atlas.kdl` from the current working directory.
A minimal config:

```kdl
canvas {
    background-color "#1a1a2e"
}

decoration {
    border-width 3.0
    border-radius 0.0
    active-color "#6699ff"
    inactive-color "#4a4a4a"
}
```

## Controls (winit backend)

| Input | Action |
|-------|--------|
| **Mod+Enter** | Spawn terminal (fish, gnome-terminal, alacritty, kitty, foot, weston-terminal, xterm) |
| **Mod+Q** | Close focused window |
| **Mod+Left-Click** | Drag window |
| **Mod+Right-Click** | Resize window |
| **Mod+Arrow** | Nudge focused window 20 canvas-units |
| **Arrow keys** | Pan viewport |
| **Plain Left-Click** | Focus window |

The **Mod** key is the **Super/Windows** key (evdev 125).

## Why "Atlas"?

Like the Titan who holds up the sky, Atlas holds your workspace — a
potentially infinite canvas that doesn't constrain windows to the edges
of physical displays.

## License

GPL-3.0-or-later
