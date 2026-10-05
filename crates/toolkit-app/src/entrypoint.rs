//! The command line, logging, and the native window: everything that runs
//! before the first frame, and what the person sees when that fails.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use eframe::egui;
use toolkit_core::integrations;

use crate::backend::Backend;
use crate::updates::{self, Updates};
use crate::window::{Mode, Opening, Window};
use crate::worker::Handle;
use crate::{demo, theme};

/// The command line. `--version` prints `claude-toolkit <version>`, which
/// fastframe's update helper checks before it installs a download.
#[derive(Debug, Parser)]
#[command(
    name = "claude-toolkit",
    version,
    about = "MCP setup for Claude Desktop"
)]
pub struct Cli {
    /// Log at debug level.
    #[arg(long)]
    pub verbose: bool,
    /// Made-up integrations in a throwaway folder; nothing is sent or stored.
    #[arg(long)]
    pub demo: bool,
    /// Draw the demo window, save it to this PNG, then quit.
    #[arg(long, value_name = "PNG")]
    pub demo_shot: Option<PathBuf>,
    /// With --demo-shot: draw it in the light theme.
    #[arg(long, requires = "demo_shot")]
    pub light: bool,
    /// With --demo or --demo-shot: the integration to select (jira, github, ...).
    #[arg(long, value_name = "KEY")]
    pub select: Option<String>,
    /// With --demo or --demo-shot: open the selected integration's setup form.
    #[arg(long)]
    pub form: bool,
    /// With --demo or --demo-shot: open the settings.
    #[arg(long, conflicts_with = "form")]
    pub settings: bool,
}

impl Cli {
    pub fn mode(&self) -> Mode {
        match &self.demo_shot {
            Some(path) => Mode::Shot {
                path: path.clone(),
                light: self.light,
            },
            None if self.demo => Mode::Demo,
            None => Mode::Normal,
        }
    }

    pub fn opening(&self) -> Result<Opening, String> {
        let select = match &self.select {
            None => None,
            Some(key) => Some(
                integrations::by_key(key)
                    .map(|integration| integration.key)
                    .ok_or_else(|| format!("no integration is called \"{key}\""))?,
            ),
        };
        Ok(Opening {
            select,
            form: self.form,
            settings: self.settings,
        })
    }
}

/// Runs the app, and tells the person why when it cannot start. It is
/// started from Finder or the Start menu, where stderr goes nowhere: without
/// this a failed start is a window that never appears.
pub fn run() -> ExitCode {
    // First: run fastframe's update helper when asked, then exit.
    let launch = fastframe_update::intercept(&updates::CONFIG);
    // Exits here for --help, --version and a mistyped flag.
    let cli = Cli::parse_from(&launch.arguments);
    let mode = cli.mode();
    let demo = mode != Mode::Normal;
    start_logging(demo, cli.verbose);
    log::info!(
        "Claude Toolkit {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match cli
        .opening()
        .map_err(anyhow::Error::msg)
        .and_then(|opening| open_window(mode, &opening, launch.receipt))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = startup_error_message(&error);
            log::error!("{message}");
            eprintln!("{message}");
            if !demo {
                show_alert(&message);
            }
            ExitCode::FAILURE
        }
    }
}

/// The text of the startup error, with every cause in the chain.
pub fn startup_error_message(error: &anyhow::Error) -> String {
    format!("Claude Toolkit could not start: {error:#}")
}

/// A folder of its own for `--demo` and `--demo-shot`, so what the demo does
/// never reaches the person's Claude Desktop.
fn demo_folder() -> PathBuf {
    let folder = std::env::temp_dir().join(format!("claude-toolkit-demo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

fn log_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "jetrockets", "claude-toolkit")
        .map(|dirs| dirs.data_local_dir().join("logs"))
}

pub fn options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Claude Toolkit")
            .with_inner_size([960.0, 660.0])
            .with_min_inner_size([760.0, 520.0])
            // An empty icon keeps the bundle's: without one eframe sets its
            // own egui logo as the application icon.
            .with_icon(egui::IconData::default()),
        ..Default::default()
    }
}

fn open_window(
    mode: Mode,
    opening: &Opening,
    receipt: Option<fastframe_update::Receipt>,
) -> anyhow::Result<()> {
    let demo = mode != Mode::Normal;
    let backend = if demo {
        demo::backend(&demo_folder())
    } else {
        Backend::real(log_dir())?
    };
    let opening = opening.clone();
    eframe::run_native(
        "Claude Toolkit",
        options(),
        Box::new(move |cc| {
            // A picture must not depend on the fonts this machine has.
            theme::install(&cc.egui_ctx, !matches!(mode, Mode::Shot { .. }));
            // Tells the helper the new version started, so it keeps it
            // instead of rolling back.
            if let Some(receipt) = receipt {
                std::thread::spawn(move || {
                    if let Err(error) = receipt.acknowledge() {
                        log::warn!("could not acknowledge the update: {error:#}");
                    }
                });
            }
            let ctx = cc.egui_ctx.clone();
            let worker = Handle::spawn(backend, Box::new(move || ctx.request_repaint()));
            let updates = if demo {
                Updates::disabled()
            } else {
                Updates::spawn(cc.egui_ctx.clone())
            };
            Ok(Box::new(Window::new(worker, updates, mode, &opening)))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))
}

/// A native alert, the one place a failed start can be seen.
fn show_alert(message: &str) {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        use objc2_app_kit::{NSAlert, NSApplication};
        use objc2_foundation::NSString;
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated, reason = "its replacement needs macOS 14")]
        app.activateIgnoringOtherApps(true);
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("Claude Toolkit could not start"));
        alert.setInformativeText(&NSString::from_str(message));
        alert.runModal();
    }
    let _ = message;
}

/// A log file people can attach to a bug report, and a panic log without
/// payloads (fastframe-log). Tokens never reach either: they travel as
/// `Secret`. `--demo` logs to the terminal only.
fn start_logging(demo: bool, verbose: bool) {
    let filter = if verbose {
        "warn,claude_toolkit=debug,toolkit_core=debug"
    } else {
        "warn,claude_toolkit=info,toolkit_core=info"
    };
    let mut logging =
        fastframe_log::Logging::new("claude-toolkit", env!("CARGO_PKG_VERSION")).filter(filter);
    if !demo
        && let Some(folder) = log_dir()
        && std::fs::create_dir_all(&folder).is_ok()
    {
        logging = logging
            .file(folder.join("claude-toolkit.log"))
            .panic_log(folder.join("panics.log"));
    }
    if let Err(error) = logging.init() {
        eprintln!("could not start logging: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("claude-toolkit").chain(arguments.iter().copied()))
    }

    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn version_names_what_the_update_helper_expects() {
        use clap::CommandFactory;
        let command = Cli::command();
        assert_eq!(command.get_name(), updates::CONFIG.slug);
        assert_eq!(
            command.render_version().trim(),
            format!(
                "{} {}",
                updates::CONFIG.slug,
                updates::CONFIG.current_version
            )
        );
    }

    #[test]
    fn each_flag_picks_its_mode() {
        assert_eq!(cli(&[]).unwrap().mode(), Mode::Normal);
        assert_eq!(cli(&["--demo"]).unwrap().mode(), Mode::Demo);
        assert_eq!(
            cli(&["--demo-shot", "a.png", "--light"]).unwrap().mode(),
            Mode::Shot {
                path: PathBuf::from("a.png"),
                light: true
            }
        );
    }

    #[test]
    fn a_flag_that_is_wrong_is_refused_not_ignored() {
        // --demo-shot without a file would start the real app on the real config.
        assert!(cli(&["--demo-shot"]).is_err());
        assert!(cli(&["--light"]).is_err());
        assert!(cli(&["--dmeo"]).is_err());
        assert!(cli(&["--demo", "--form", "--settings"]).is_err());
    }

    #[test]
    fn the_opening_names_a_real_integration() {
        let opening = cli(&["--demo", "--select", "github", "--form"])
            .unwrap()
            .opening()
            .unwrap();
        assert_eq!(
            opening,
            Opening {
                select: Some("github"),
                form: true,
                settings: false
            }
        );
        assert!(
            cli(&["--demo", "--select", "trello"])
                .unwrap()
                .opening()
                .is_err()
        );
    }

    #[test]
    fn the_startup_error_names_every_cause() {
        let error = anyhow::anyhow!("no OpenGL context").context("opening the window");
        assert_eq!(
            startup_error_message(&error),
            "Claude Toolkit could not start: opening the window: no OpenGL context"
        );
    }

    #[test]
    fn the_window_keeps_the_bundles_icon() {
        let icon = options().viewport.icon.expect("an icon");
        assert_eq!(*icon, egui::IconData::default());
    }
}
