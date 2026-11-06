//! CLI entrypoint for `git-axolotl`.
//!
//! Commands:
//! - `git-axolotl collect <login> [--window annual|rolling] [--output snapshot.json]`
//! - `git-axolotl render <snapshot.json> [--output-dir axolotl] [--view side|dive|top] [--natural]`
//! - `git-axolotl demo [--commits 2800] [--output-dir axolotl] [--view side|dive|top] [--natural]`

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::NaiveDate;
use git_axolotl::coat::AxolotlCoat;
use git_axolotl::collect::collect_account_snapshot_with_window;
use git_axolotl::history::{AccountSnapshot, ContributionWindow, LanguageShare};
use git_axolotl::output::{encode_gif, encode_png, encode_preview_html, encode_share_card_png};
use git_axolotl::render::{
    render_share_card_frame, render_still_frame, render_swim_loop, AxolotlBiology, CameraView,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run_cli(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("git-axolotl: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli(args: &[String]) -> Result<(), String> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err(usage_text());
    };

    match subcommand {
        "collect" => {
            let login = args
                .get(1)
                .ok_or_else(|| "missing <login> for `collect`".to_owned())?;
            let mut output_path: Option<PathBuf> = None;
            let mut window = ContributionWindow::Annual;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--output" | "-o" => {
                        let p = args.get(i + 1).ok_or("missing path after --output")?;
                        output_path = Some(PathBuf::from(p));
                        i += 2;
                    }
                    "--window" | "-w" => {
                        let w = args.get(i + 1).ok_or("missing window after --window")?;
                        window = parse_window(w)?;
                        i += 2;
                    }
                    other => return Err(format!("unknown option `{other}`")),
                }
            }
            let snapshot = collect_account_snapshot_with_window(login, window)?;
            let json = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
            if let Some(path) = output_path {
                fs::write(&path, format!("{json}\n"))
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                println!("Saved account snapshot for @{login} to {}", path.display());
            } else {
                println!("{json}");
            }
            Ok(())
        }
        "render" => {
            let snapshot_file = args
                .get(1)
                .ok_or_else(|| "missing <snapshot.json> for `render`".to_owned())?;
            let mut output_dir = PathBuf::from("axolotl");
            let mut view = CameraView::Side;
            let mut natural_only = false;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--output-dir" | "-o" => {
                        let p = args.get(i + 1).ok_or("missing dir after --output-dir")?;
                        output_dir = PathBuf::from(p);
                        i += 2;
                    }
                    "--view" => {
                        let v = args.get(i + 1).ok_or("missing view after --view")?;
                        view = parse_view(v)?;
                        i += 2;
                    }
                    "--natural" => {
                        natural_only = true;
                        i += 1;
                    }
                    other => return Err(format!("unknown option `{other}`")),
                }
            }

            let raw =
                fs::read_to_string(snapshot_file).map_err(|e| format!("{snapshot_file}: {e}"))?;
            let snapshot: AccountSnapshot =
                serde_json::from_str(&raw).map_err(|e| format!("{snapshot_file}: {e}"))?;
            render_to_directory(&snapshot, &output_dir, view, natural_only)
        }
        "demo" => {
            let mut commits = 2800_u64;
            let mut output_dir = PathBuf::from("axolotl");
            let mut view = CameraView::Side;
            let mut natural_only = false;

            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--commits" | "-c" => {
                        let c = args.get(i + 1).ok_or("missing number after --commits")?;
                        commits = c.parse::<u64>().map_err(|_| "invalid commit count")?;
                        i += 2;
                    }
                    "--output-dir" | "-o" => {
                        let p = args.get(i + 1).ok_or("missing dir after --output-dir")?;
                        output_dir = PathBuf::from(p);
                        i += 2;
                    }
                    "--view" => {
                        let v = args.get(i + 1).ok_or("missing view after --view")?;
                        view = parse_view(v)?;
                        i += 2;
                    }
                    "--natural" => {
                        natural_only = true;
                        i += 1;
                    }
                    other => return Err(format!("unknown option `{other}`")),
                }
            }

            let demo_snapshot = AccountSnapshot {
                login: "demo-user".to_owned(),
                account_id: 42,
                observed_on: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                total_contributions: commits,
                monthly_contributions: commits,
                current_streak: 14,
                longest_streak: 30,
                languages: vec![
                    LanguageShare {
                        name: "TypeScript".to_owned(),
                        hex_color: "#3178c6".to_owned(),
                        repo_count: 10,
                    },
                    LanguageShare {
                        name: "Rust".to_owned(),
                        hex_color: "#dea584".to_owned(),
                        repo_count: 8,
                    },
                    LanguageShare {
                        name: "Python".to_owned(),
                        hex_color: "#3572A5".to_owned(),
                        repo_count: 5,
                    },
                ],
            };
            render_to_directory(&demo_snapshot, &output_dir, view, natural_only)
        }
        _ => Err(usage_text()),
    }
}

fn parse_view(s: &str) -> Result<CameraView, String> {
    match s.to_lowercase().as_str() {
        "side" => Ok(CameraView::Side),
        "dive" => Ok(CameraView::Dive),
        "top" => Ok(CameraView::Top),
        _ => Err(format!(
            "unknown view `{s}` (expected `side`, `dive`, or `top`)"
        )),
    }
}

fn parse_window(s: &str) -> Result<ContributionWindow, String> {
    match s.to_lowercase().as_str() {
        "annual" | "year" => Ok(ContributionWindow::Annual),
        "rolling" | "rolling365" | "365" => Ok(ContributionWindow::Rolling365),
        _ => Err(format!(
            "unknown window `{s}` (expected `annual` or `rolling`)"
        )),
    }
}

fn render_to_directory(
    snapshot: &AccountSnapshot,
    output_dir: &Path,
    view: CameraView,
    natural_only: bool,
) -> Result<(), String> {
    let coat = AxolotlCoat::from_snapshot(snapshot, natural_only);
    let swim_frames = render_swim_loop(snapshot, &coat, view);

    let gif_bytes = encode_gif(&swim_frames)?;
    let png_bytes = encode_png(&render_still_frame(snapshot, &coat, view))?;
    let share_card_bytes = encode_share_card_png(&render_share_card_frame(snapshot, &coat, view))?;

    let bio = AxolotlBiology::from_commits(snapshot.total_contributions);
    let caption = format!(
        "@{} - {} ({} contributions)",
        snapshot.login,
        bio.stage_name(),
        bio.commits
    );
    let preview_html = encode_preview_html(&caption);

    fs::create_dir_all(output_dir).map_err(|e| format!("{}: {e}", output_dir.display()))?;
    let gif_path = output_dir.join("axolotl.gif");
    let png_path = output_dir.join("axolotl.png");
    let share_path = output_dir.join("share-card.png");
    let html_path = output_dir.join("preview.html");

    fs::write(&gif_path, gif_bytes).map_err(|e| format!("{}: {e}", gif_path.display()))?;
    fs::write(&png_path, png_bytes).map_err(|e| format!("{}: {e}", png_path.display()))?;
    fs::write(&share_path, share_card_bytes)
        .map_err(|e| format!("{}: {e}", share_path.display()))?;
    fs::write(&html_path, preview_html.as_bytes())
        .map_err(|e| format!("{}: {e}", html_path.display()))?;

    println!(
        "{caption}: wrote {}, {}, {}, {}",
        gif_path.display(),
        png_path.display(),
        share_path.display(),
        html_path.display()
    );
    Ok(())
}

fn usage_text() -> String {
    "Usage:\n  \
     git-axolotl collect <login> [--window annual|rolling] [--output snapshot.json]\n  \
     git-axolotl render <snapshot.json> [--output-dir axolotl] [--view side|dive|top] [--natural]\n  \
     git-axolotl demo [--commits 2800] [--output-dir axolotl] [--view side|dive|top] [--natural]"
        .to_owned()
}
