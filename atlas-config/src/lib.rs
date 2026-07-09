use std::path::Path;

use knuffel::{Decode, parse};
use thiserror::Error;

/// Validate that the `kdl` crate compiles and can be used for low-level KDL parsing.
/// This is kept minimal — knuffel is our primary parser for derive-based config loading.
pub fn parse_kdl_document(input: &str) -> Result<Vec<kdl::KdlNode>, kdl::KdlError> {
    kdl::parse_document(input)
}

// ─── Error ────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse config")]
    Parse(#[from] knuffel::Error),
}

// ─── Raw KDL config structs ───────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlConfig {
    #[knuffel(child)]
    pub canvas: Option<KdlCanvas>,

    #[knuffel(children(name = "output"))]
    pub outputs: Vec<KdlOutput>,

    #[knuffel(children(name = "region"))]
    pub regions: Vec<KdlRegion>,

    #[knuffel(children(name = "binding"))]
    pub bindings: Vec<KdlBinding>,

    #[knuffel(child)]
    pub animation: Option<KdlAnimation>,

    #[knuffel(children(name = "plugin"))]
    pub plugins: Vec<KdlPlugin>,

    #[knuffel(children(name = "bookmark"))]
    pub bookmarks: Vec<KdlBookmark>,
}

// ── Canvas ────────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlCanvas {
    #[knuffel(child, unwrap(argument))]
    pub background_color: Option<String>,
}

// ── Output ────────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlOutput {
    #[knuffel(argument)]
    pub name: String,

    #[knuffel(child)]
    pub position: Option<KdlPosition>,

    #[knuffel(child, unwrap(argument))]
    pub scale: Option<f64>,

    #[knuffel(child)]
    pub mode: Option<KdlMode>,
}

#[derive(Debug, Decode)]
pub struct KdlPosition {
    #[knuffel(argument)]
    pub x: i32,
    #[knuffel(argument)]
    pub y: i32,
}

#[derive(Debug, Decode)]
pub struct KdlMode {
    #[knuffel(argument)]
    pub width: i32,
    #[knuffel(argument)]
    pub height: i32,
    #[knuffel(argument)]
    pub refresh: i32,
}

// ── Region ────────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlRegion {
    #[knuffel(argument)]
    pub name: String,

    #[knuffel(child)]
    pub rect: Option<KdlRect>,

    #[knuffel(child, unwrap(argument))]
    pub anchor: Option<String>,

    #[knuffel(child, unwrap(argument))]
    pub layout: Option<String>,
}

#[derive(Debug, Decode)]
pub struct KdlRect {
    #[knuffel(argument)]
    pub x: i32,
    #[knuffel(argument)]
    pub y: i32,
    #[knuffel(argument)]
    pub width: i32,
    #[knuffel(argument)]
    pub height: i32,
}

// ── Binding ───────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlBinding {
    #[knuffel(argument)]
    pub combo: String,

    #[knuffel(property)]
    pub action: Option<String>,

    #[knuffel(property)]
    pub args: Option<String>,
}

// ── Animation ─────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlAnimation {
    #[knuffel(child, unwrap(argument))]
    pub default_ease: Option<String>,

    #[knuffel(child, unwrap(argument))]
    pub duration_ms: Option<u64>,
}

// ── Plugin ────────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlPlugin {
    #[knuffel(argument)]
    pub path: String,
}

// ── Bookmark ─────────────────────────────────────────────────────────

#[derive(Debug, Decode)]
pub struct KdlBookmark {
    #[knuffel(argument)]
    pub name: String,
    #[knuffel(argument)]
    pub x: i32,
    #[knuffel(argument)]
    pub y: i32,
}

// ─── Runtime config (normalized) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub canvas: CanvasConfig,
    pub outputs: Vec<OutputConfig>,
    pub regions: Vec<RegionConfig>,
    pub bindings: Vec<BindingConfig>,
    pub animation: AnimationConfig,
    pub plugins: Vec<PluginConfig>,
    pub bookmarks: Vec<BookmarkConfig>,
}

#[derive(Debug, Clone)]
pub struct CanvasConfig {
    pub background_color: String,
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub mode: Option<(i32, i32, i32)>,
}

#[derive(Debug, Clone)]
pub struct RegionConfig {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub anchor: String,
    pub layout: String,
}

#[derive(Debug, Clone)]
pub struct BindingConfig {
    pub combo: String,
    pub action: String,
    pub args: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AnimationConfig {
    pub default_ease: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PluginConfig {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct BookmarkConfig {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

// ─── Defaults ─────────────────────────────────────────────────────────

impl Default for CanvasConfig {
    fn default() -> Self {
        CanvasConfig {
            background_color: "#1a1a2e".into(),
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            default_ease: "spring".into(),
            duration_ms: 200,
        }
    }
}

// ─── Parse ────────────────────────────────────────────────────────────

pub fn parse_config<P: AsRef<Path>>(path: P) -> Result<RuntimeConfig, ConfigError> {
    let source = std::fs::read_to_string(path.as_ref())?;
    let kdl: KdlConfig = parse(path.as_ref().to_string_lossy().as_ref(), &source)?;
    normalize(kdl)
}

pub fn parse_config_str(name: &str, source: &str) -> Result<RuntimeConfig, ConfigError> {
    let kdl: KdlConfig = parse(name, source)?;
    normalize(kdl)
}

fn normalize(kdl: KdlConfig) -> Result<RuntimeConfig, ConfigError> {
    let canvas = kdl
        .canvas
        .map(|c| CanvasConfig {
            background_color: c.background_color.unwrap_or_else(|| "#1a1a2e".into()),
        })
        .unwrap_or_default();

    let outputs: Vec<OutputConfig> = kdl
        .outputs
        .into_iter()
        .map(|o| OutputConfig {
            name: o.name,
            x: o.position.as_ref().map(|p| p.x).unwrap_or(0),
            y: o.position.as_ref().map(|p| p.y).unwrap_or(0),
            scale: o.scale.unwrap_or(1.0),
            mode: o.mode.map(|m| (m.width, m.height, m.refresh)),
        })
        .collect();

    let regions: Vec<RegionConfig> = kdl
        .regions
        .into_iter()
        .map(|r| RegionConfig {
            name: r.name,
            x: r.rect.as_ref().map(|r| r.x as f64).unwrap_or(0.0),
            y: r.rect.as_ref().map(|r| r.y as f64).unwrap_or(0.0),
            width: r.rect.as_ref().map(|r| r.width as f64).unwrap_or(1920.0),
            height: r.rect.as_ref().map(|r| r.height as f64).unwrap_or(1080.0),
            anchor: r.anchor.unwrap_or_else(|| "center".into()),
            layout: r.layout.unwrap_or_else(|| "floating".into()),
        })
        .collect();

    let bindings: Vec<BindingConfig> = kdl
        .bindings
        .into_iter()
        .map(|b| BindingConfig {
            combo: b.combo,
            action: b.action.unwrap_or_else(|| "none".into()),
            args: b.args,
        })
        .collect();

    let animation = kdl
        .animation
        .map(|a| AnimationConfig {
            default_ease: a.default_ease.unwrap_or_else(|| "spring".into()),
            duration_ms: a.duration_ms.unwrap_or(200),
        })
        .unwrap_or_default();

    let plugins: Vec<PluginConfig> = kdl
        .plugins
        .into_iter()
        .map(|p| PluginConfig { path: p.path })
        .collect();

    let bookmarks: Vec<BookmarkConfig> = kdl
        .bookmarks
        .into_iter()
        .map(|b| BookmarkConfig {
            name: b.name,
            x: b.x as f64,
            y: b.y as f64,
        })
        .collect();

    Ok(RuntimeConfig {
        canvas,
        outputs,
        regions,
        bindings,
        animation,
        plugins,
        bookmarks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_kdl_err(e: &knuffel::Error) {
        eprintln!("KDL ERROR: {e:#?}");
    }

    #[test]
    fn parse_minimal() {
        let cfg = parse_config_str("test.kdl", "").unwrap();
        assert_eq!(cfg.canvas.background_color, "#1a1a2e");
        assert!(cfg.outputs.is_empty());
        assert!(cfg.bindings.is_empty());
        assert_eq!(cfg.animation.default_ease, "spring");
    }

    #[test]
    fn parse_simple_binding() {
        let result = parse_config_str("test.kdl", r#"binding "Mod+Q" action="close""#);
        assert!(result.is_ok(), "binding: {:?}", result.err());
    }

    #[test]
    fn parse_canvas_only() {
        let result = parse_config_str("test.kdl", "canvas { }");
        assert!(result.is_ok(), "canvas empty: {:?}", result.err());
    }

    #[test]
    fn parse_canvas_with_bg() {
        let src = "canvas {\n    background-color \"#0f0f23\"\n}";
        let result = parse_config_str("test.kdl", src);
        if let Err(ConfigError::Parse(err)) = &result {
            show_kdl_err(err);
        }
        assert!(result.is_ok(), "canvas bg: {:?}", result.err());
    }

    #[test]
    fn parse_output_only() {
        let src = "output \"eDP-1\" {\n    position 0 0;\n    scale 1.0;\n    mode 1920 1080 60000\n}";
        let result = parse_config_str("test.kdl", src);
        assert!(result.is_ok(), "output: {:?}", result.err());
    }

    #[test]
    fn parse_region_only() {
        let src = "region \"main\" {\n    rect 0 0 1920 1080;\n    anchor \"center\";\n    layout \"floating\"\n}";
        let result = parse_config_str("test.kdl", src);
        assert!(result.is_ok(), "region: {:?}", result.err());
    }

    #[test]
    fn parse_animation_only() {
        let src = "animation {\n    default-ease \"spring\";\n    duration-ms 150\n}";
        let result = parse_config_str("test.kdl", src);
        if let Err(ConfigError::Parse(err)) = &result {
            show_kdl_err(err);
        }
        assert!(result.is_ok(), "animation: {:?}", result.err());
    }

    #[test]
    fn parse_full_config() {
        let kdl = r##"
canvas {
    background-color "#0f0f23"
}

output "eDP-1" {
    position 0 0
    scale 1.0
    mode 1920 1080 60000
}

region "main" {
    rect 0 0 1920 1080
    anchor "center"
    layout "floating"
}

binding "Mod+Q" action="close"
binding "Mod+Return" action="exec" args="alacritty"

animation {
    default-ease "spring"
    duration-ms 150
}

plugin "/usr/lib/atlas/bar.wasm"

bookmark "origin" 0 0
"##;

        let result = parse_config_str("test.kdl", kdl);
        if let Err(ConfigError::Parse(err)) = &result {
            show_kdl_err(err);
        }
        let cfg = result.unwrap();
        assert_eq!(cfg.canvas.background_color, "#0f0f23");
        assert_eq!(cfg.outputs.len(), 1);
        assert_eq!(cfg.outputs[0].name, "eDP-1");
        assert_eq!(cfg.outputs[0].x, 0);
        assert_eq!(cfg.outputs[0].scale, 1.0);
        assert_eq!(cfg.regions.len(), 1);
        assert_eq!(cfg.regions[0].name, "main");
        assert_eq!(cfg.regions[0].anchor, "center");
        assert_eq!(cfg.bindings.len(), 2);
        assert_eq!(cfg.bindings[0].combo, "Mod+Q");
        assert_eq!(cfg.bindings[0].action, "close");
        assert_eq!(cfg.bindings[1].args.as_deref(), Some("alacritty"));
        assert_eq!(cfg.animation.duration_ms, 150);
        assert_eq!(cfg.plugins.len(), 1);
        assert_eq!(cfg.plugins[0].path, "/usr/lib/atlas/bar.wasm");
        assert_eq!(cfg.bookmarks.len(), 1);
        assert_eq!(cfg.bookmarks[0].name, "origin");
    }

    #[test]
    fn parse_errors_with_bad_config() {
        let result = parse_config_str("bad.kdl", "output {");
        assert!(result.is_err());
    }
}
