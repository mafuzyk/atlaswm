// TUI setup configurator — launched via `atlasfetch setup`.
//
// Built with ratatui + crossterm. Uses a step-based navigation flow with a
// live preview panel that updates as each setting changes.
//
// Design principles:
//   - Settings on the left, live preview on the right
//   - Keyboard navigation with tab/arrows, plus mouse support
//   - Every change reflects immediately in the preview
//   - Step indicator at the top showing progress

mod app;

pub use app::run;
