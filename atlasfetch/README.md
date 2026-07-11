<div align="center">

# atlasfetch

**Centered ASCII art with powerline panels.**  
Zero dependencies · Pure Python · Linux

[![Python](https://img.shields.io/badge/python-%E2%89%A53.6-3776AB?style=flat-square&logo=python&logoColor=white)](https://python.org)
[![License](https://img.shields.io/badge/license-GPL--3.0-8A2BE2?style=flat-square)](LICENSE)
[![Status](https://img.shields.io/badge/status-stable-22AA66?style=flat-square)]()
<br>
[Features](#features) · [Install](#install) · [Usage](#usage) · [Customization](#customization) · [Comparison](#comparison) · [Roadmap](#roadmap)

<br>

<img src="" alt="atlasfetch screenshot" width="720">

</div>

---

atlasfetch is a system information tool designed to accompany [atlasWM](https://github.com/mafuzyk/atlaswm), a Wayland compositor built around an infinite canvas. It shares the same aesthetic priorities: centered layouts, powerline separators, and visual balance.

It displays your distro's ASCII logo centered on the terminal, with powerline-styled info panels on both sides. It is not a neofetch or fastfetch competitor — it simply provides a fetch tool that matches the atlasWM look. If you want a standalone general-purpose fetch, those projects are better choices.

It runs on any Linux distro with Python ≥ 3.6, no matter your window manager or desktop environment. No pip, no dependencies, no compilation.

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

**Zero dependencies** — pure Python standard library. Reads `/proc`, `/sys`, and `pci.ids`. No pip install, no virtualenv, no compilation.

</td>
<td width="50%">

**First-run wizard** — pick a color palette and ASCII logo on first launch. No need to edit config files before seeing results.

</td>
</tr>
<tr>
<td width="50%">

**25 color presets** — LGBTQ+ flags (trans, pan, bi, ace, lesbian, gay, aromantic, agender, nb, genderfluid, intersex) and themes (catppuccin, dracula, gruvbox, nord, tokyonight, rose-pine, monokai, and more).

</td>
<td width="50%">

**Custom palettes** — create your own color schemes through the wizard or directly in `config.json`. They persist and are listed alongside built-in presets.

</td>
</tr>
<tr>
<td width="50%">

**Multi-distro package counting** — detects packages from pacman, dpkg, rpm, xbps, apk, emerge, nix-store, flatpak, snap, and more.

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

- **Python** ≥ 3.6 (standard library only — no pip packages)
- **Nerd Font** (optional — for panel icons)
- **pci.ids** (optional — for descriptive GPU names; falls back to vendor hex)

<br>

<details open>
<summary><b>Quick install</b> — any distro, one command</summary>

```bash
curl -sSL https://raw.githubusercontent.com/mafuzyk/atlasfetch/main/atlasfetch \
  -o ~/.local/bin/atlasfetch && chmod +x ~/.local/bin/atlasfetch
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

<details>
<summary><b>Git clone</b> — development or offline use</summary>

```bash
git clone https://github.com/mafuzyk/atlasfetch.git ~/.local/share/atlasfetch
ln -sf ~/.local/share/atlasfetch/atlasfetch ~/.local/bin/atlasfetch
```

</details>

<br>

<details>
<summary><b>Manual</b></summary>

```bash
wget https://raw.githubusercontent.com/mafuzyk/atlasfetch/main/atlasfetch
chmod +x atlasfetch
sudo mv atlasfetch /usr/local/bin/
```

</details>

<br>

### First run

```bash
atlasfetch
```

On first launch, the interactive setup asks for a color palette and ASCII logo. After that, it renders system info immediately.

To reopen the wizard later:

```bash
atlasfetch -i
```

---

## Usage

```
atlasfetch              Render system info
atlasfetch -i           Open setup wizard
atlasfetch --preset <n> Apply a preset palette and exit
atlasfetch --list-presets List all presets with color swatches
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

**Set a theme without the wizard:**
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

Place any ASCII art in `~/.config/atlasfetch/logo.txt`. The tool picks it up automatically. The wizard can also select from the 18 built-in distro logos.

### Custom palettes

Add your own color schemes in `config.json`:

```json
{
  "custom_palettes": {
    "my-theme": ["#ff0000", "#00ff00", "#0000ff"]
  }
}
```

The wizard includes a custom palette creator — choose `c` at the palette prompt and type hex colors separated by spaces.

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

## Comparison

atlasfetch is not a competitor to these projects — it solves a different problem. This table is provided for reference, not comparison shopping.

| | atlasfetch | fastfetch | neofetch |
|---|---|---|---|
| **Layout** | Centered ASCII with side panels | Left-aligned, customizable | Left-aligned, customizable |
| **Dependencies** | None (Python stdlib) | Compiled C++, ~30 libraries | Bash, optional dependencies |
| **Install size** | ~40 KB (single script) | ~2 MB (binary) | ~1 MB (script + logos) |
| **Logo count** | 18 distro logos | ~180 logos | ~160 logos |
| **Configuration** | `config.json` | `config.jsonc` | `config.conf` |
| **Speed** | Fast (pure Python) | Faster (compiled C) | Moderate (Bash subshells) |
| **Setup wizard** | Yes | No | No |
| **Powerline support** | Native | Requires theme | Requires theme |
| **Language** | Python 3 | C | Bash |
| **Platform** | Linux only | Linux, macOS, Windows, BSD | Linux, macOS, BSD |

---

## Philosophy

atlasfetch exists primarily as a companion to atlasWM. It was built to match a specific visual language — centered, symmetrical, with powerline separators — not to compete with general-purpose fetch tools.

**What atlasfetch is not:**

- It is not a neofetch or fastfetch replacement.
- It is not a comprehensive system diagnostic tool.
- It is not written for maximum performance or maximum logo count.

**What atlasfetch is:**

- A fetch tool designed for the atlasWM aesthetic.
- A single Python script you can read, understand, and modify.
- Zero dependencies beyond the standard library.
- A tool that respects terminal width — when the screen is too narrow, the logo steps out of the way.

---

## Roadmap

- [x] Centered ASCII art with 18 distro logos
- [x] Powerline left/right panels with auto-truncation
- [x] Interactive first-run wizard
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
atlasfetch          → single Python script
├── ATLAS_LOGO      → default ASCII (Arch Linux)
├── 18 distro logos → one file per logo
├── 25 presets      → color schemes
├── DEFAULT_CFG     → default configuration
├── _collect_info() → system field gathering
├── render()        → layout engine
├── _run_setup()    → wizard
└── main()          → CLI dispatch
```

---

<div align="center">

**License** — [GPL-3.0-or-later](LICENSE) ·
**Repository** — [github.com/mafuzyk/atlasfetch](https://github.com/mafuzyk/atlasfetch) ·
**Related** — [atlasWM](https://github.com/mafuzyk/atlaswm)

<br>

Contributions, issues, and logo submissions are welcome.  
If you find this useful, consider leaving a star.

<sub><sup>AI used exclusively for code review and commit messages.</sup></sub>

</div>
