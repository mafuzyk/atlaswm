<div align="center">

# atlasfetch

**Centered ASCII art with powerline panels.**  
Single binary · Linux · Written in Rust

[![Rust](https://img.shields.io/badge/rust-1.85+-DEA584?style=flat-square&logo=rust&logoColor=white)](https://rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--3.0-8A2BE2?style=flat-square)](LICENSE)
[![Status](https://img.shields.io/badge/status-stable-22AA66?style=flat-square)]()
<br>
[Features](#features) · [Install](#install) · [Usage](#usage) · [Customization](#customization) · [Design Philosophy](#design-philosophy) · [Roadmap](#roadmap)

<br>

<img src="" alt="atlasfetch screenshot" width="720">

</div>

---

atlasfetch is a system information tool designed to accompany [atlasWM](https://github.com/mafuzyk/atlaswm), a Wayland compositor built around an infinite canvas. It shares the same aesthetic priorities: centered layouts, powerline separators, and visual balance.

It displays your distro's ASCII logo centered on the terminal, with powerline-styled info panels on both sides. It runs on any Linux distro, no matter your window manager or desktop environment.

---

## Features

<table>
<tr>
<td width="50%">

**Centered ASCII** — distro logos aligned to the center of your terminal, not the left edge. 18 built-in logos from Alpine to Void.

</td>
<td width="50%">

**Powerline panels** — two sidebars with Nerd Font icons, automatic truncation, and cascade shift when content overflows.

</td>
</tr>
<tr>
<td width="50%">

**Single binary** — compiled Rust, no runtime dependencies. Drop it in your path and it works.

</td>
<td width="50%">

**TUI configurator** — run `atlasfetch setup` for an interactive configuration experience with live preview, theme selection, and field management.

</td>
</tr>
<tr>
<td width="50%">

**25 color presets** — LGBTQ+ flags (trans, pan, bi, ace, lesbian, gay, aromantic, agender, nb, genderfluid, intersex) and themes (catppuccin, dracula, gruvbox, nord, tokyonight, rose-pine, monokai, and more).

</td>
<td width="50%">

**Custom palettes** — create your own color schemes through the TUI or directly in `config.json`. They persist and are listed alongside built-in presets.

</td>
</tr>
<tr>
<td width="50%">

**Multi-distro package counting** — detects packages from pacman, dpkg, rpm, xbps, apk, emerge, nix-store, flatpak, and more.

</td>
<td width="50%">

**Adaptive layout** — ASCII art hides when the terminal is too narrow, keeping the info panels readable at any width.

</td>
</tr>
</table>

---

## Anatomy

```
  charlie@atlasbox
  ──────────────────────────────
                                    -`
       OS   CachyOS            .o+`            Up   10h 7m
      Usr   charlie           `ooo/            Term   kitty
     Krn   7.1.3-cachyos     `+oooo:         CPU   AMD Ryzen 3
     Pkg   1766              `+oooooo:        GPU   Radeon …
      Sh   fish              -+oooooo+:        Mem   3.2/7.6G
      WM   Hyprland        `/:-:++oooo+:       Dsk   28/58G
                                                                 `
```

---

## Install

### Requirements

- **Linux** (reads from `/proc` and `/sys`)
- **Nerd Font** (optional — for panel icons)

<br>

<details open>
<summary><b>Build from source</b> — recommended</summary>

```bash
git clone https://github.com/mafuzyk/atlasfetch.git
cd atlasfetch
cargo build --release
cp target/release/atlasfetch ~/.local/bin/
```

Make sure `~/.local/bin` is in your `PATH`:

```bash
# bash/zsh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc

# fish
fish_add_path ~/.local/bin
```

</details>

<br>

<details>
<summary><b>Nix / NixOS</b></summary>

```bash
nix run github:mafuzyk/atlasfetch            # run once
nix profile install github:mafuzyk/atlasfetch # install
```

</details>

<br>

### First run

```bash
atlasfetch
```

On first launch, atlasfetch creates a default configuration and renders system info with your distro's logo. To customize the look, open the TUI configurator:

```bash
atlasfetch setup
```

The configurator walks through six screens:

| Step | What you choose |
|------|----------------|
| **Theme** | One of 25 color presets — flag themes or curated palettes |
| **ASCII** | Your distro logo (18 included) or a custom file |
| **Layout** | How panels are positioned (Centered, Compact, Wide, Minimal, Balanced) |
| **Panels** | Which fields appear, their order, and labels |
| **Summary** | Review everything and save |

Every screen shows a live preview of how the output will look. Changes appear instantly — no save-and-reload cycle.

---

## Usage

```
atlasfetch              Render system info
atlasfetch setup        Open TUI configurator
atlasfetch --preset <n> Apply a preset palette and exit
atlasfetch --list-presets List all presets
atlasfetch -h           Show help
atlasfetch -v           Show version
```

### Run on terminal open

<details open>
<summary><b>Fish</b> — <code>~/.config/fish/config.fish</code></summary>

```fish
if status is-interactive
    atlasfetch
end
```

</details>

<details>
<summary><b>Bash</b> — <code>~/.bashrc</code></summary>

```bash
if [[ $- == *i* ]]; then
    atlasfetch
fi
```

</details>

<details>
<summary><b>Zsh</b> — <code>~/.zshrc</code></summary>

```zsh
if [[ -o interactive ]]; then
    atlasfetch
fi
```

</details>

### Workflows

**Set a theme without the TUI:**
```bash
atlasfetch --preset dracula
```

**Use a distro logo different from your OS:**
```json
// ~/.config/atlasfetch/config.json
"logo": { "key": "nixos" }
```

**Add a custom info field:**
```json
"display": {
    "left": [
        ["os",       "\uf17c", "OS"],
        ["packages", "\uf1b3", "Pkg"],
        ["load",     "\uf0e7", "Load"]
    ]
}
```

---

## Customization

Configuration lives in `~/.config/atlasfetch/config.json`, created on first run.

| Field | Description |
|-------|-------------|
| `logo.key` | Which built-in logo to display (`"arch"`, `"nixos"`, `"ubuntu"`, etc.) |
| `logo.path` | Path to a custom ASCII art file |
| `logo.colors` | Array of hex colors for the logo |
| `title.format` | Title template (`{user}@{host}`) |
| `panel.left_pad` | Left margin in spaces |
| `panel.max_shift` | Maximum cascade shift for overflowing info |
| `display.left` / `display.right` | Arrays of `[field, icon, label]` |

**Available info fields:**

`os` · `user` · `host` · `kernel` · `uptime` · `packages` · `shell` · `terminal` · `cpu` · `gpu` · `memory` · `disk` · `wm` · `load` · `processes` · `local_ip` · `resolution` · `de` · `font`

### Custom ASCII

Place any ASCII art in `~/.config/atlasfetch/logo.txt`. The tool picks it up automatically. The TUI can also select from the 18 built-in distro logos.

### Custom palettes

Add your own color schemes in `config.json`:

```json
{
  "custom_palettes": {
    "my-theme": ["#ff0000", "#00ff00", "#0000ff"]
  }
}
```

The TUI configurator includes a palette editor — open the configurator and navigate to the theme screen to add custom colors.

### Presets

Built-in color presets:

<br>

<div align="center">

| Flags | Themes |
|-------|--------|
| xenogender, trans, nb, genderfluid | arch, catppuccin-mocha, catppuccin-latte |
| pan, bi, ace, lesbian, gay | dracula, gruvbox, tokyonight, nord |
| intersex, aromantic, agender | everforest, solarized-dark, monokai |
| | one-dark, rose-pine, synthwave |

</div>

---

## Design Philosophy

atlasfetch was built around a simple idea: system information should be pleasant to look at.

Most fetch tools align text to the left edge of the terminal. atlasfetch centers everything — the ASCII art, the title, the separator — and wraps information in powerline-styled panels on both sides. This creates a calm, balanced composition that works at any terminal width.

The TUI configurator (`atlasfetch setup`) makes customization immediate and visual. Every change — theme, logo, layout, field order — updates the preview in real time. There is no edit-and-reload cycle.

The project values:

- **Simplicity** — one binary, no runtime dependencies, no package manager required.
- **Clarity** — the output reads cleanly at a glance. ASCII, title, separator, panels — no clutter.
- **Adaptability** — when the terminal is narrow, the logo steps aside and the panels remain readable.
- **Self-expression** — 25 color presets, custom palettes, custom ASCII art, and full field control.
- **Craft** — powerline separators, cascade offsets, truncation with ellipsis, careful spacing.

Atlasfetch does one thing and tries to do it well: display your system with centered elegance.

---

## Roadmap

- [x] Centered ASCII art with 18 distro logos
- [x] Powerline left/right panels with auto-truncation
- [x] TUI configurator with live preview
- [x] 25 color presets (flags + themes)
- [x] Custom palette support
- [x] Multi-distro package detection
- [x] Adaptive layout (hide ASCII on narrow terminals)
- [x] Nix flake packaging
- [ ] `--gen-config` flag to dump current config
- [ ] JSON output mode for scripting
- [ ] More distro logos (contributions welcome)
- [ ] AUR package
- [ ] Gentoo ebuild
- [ ] Custom panel field ordering presets

---

## Gallery

<div align="center">

<img src="" alt="atlasfetch Arch Linux" width="600">
<br>
<em>Default Arch logo with arch color preset</em>

<br><br>

<img src="" alt="atlasfetch NixOS" width="600">
<br>
<em>NixOS logo with catppuccin-mocha preset</em>

<br><br>

<img src="" alt="atlasfetch narrow terminal" width="400">
<br>
<em>Narrow terminal — ASCII hidden, panels remain</em>

</div>

---

## Project

```
atlasfetch           → Rust binary
├── src/             → source code
│   ├── main.rs      → entry point
│   ├── cli.rs       → argument parsing
│   ├── config.rs    → configuration model
│   ├── info.rs      → system info collection
│   ├── render.rs    → layout engine
│   ├── theme.rs     → color presets
│   ├── ascii.rs     → logo loading
│   ├── layout.rs    → layout definitions
│   └── tui/         → TUI configurator
│       ├── mod.rs
│       └── app.rs
├── logos/           → 18 distro ASCII arts
├── flake.nix        → Nix packaging
└── Cargo.toml       → dependencies
```

---

<div align="center">

**License** — [GPL-3.0-or-later](LICENSE) ·
**Repository** — [github.com/mafuzyk/atlasfetch](https://github.com/mafuzyk/atlasfetch) ·
**Related** — [atlasWM](https://github.com/mafuzyk/atlaswm)

<br>

Contributions, issues, and logo submissions are welcome.  
If you find this useful, consider leaving a star.

</div>
