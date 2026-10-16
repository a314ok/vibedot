//! `vibedot` - the Vibedot editor and command-line tool.
//!
//! ```text
//! vibedot                                  open the project manager
//! vibedot <project-dir>                    open a project
//! vibedot new <dir> [--name N] [--template neon|empty]
//! vibedot run <project-dir>                run the game in the player
//! vibedot pack <project-dir> <out.vpk>     pack project files
//! vibedot export <project-dir> [--out dist] [--platform linux|windows|macos] [--player path]
//! vibedot check <project-dir>              compile every blueprint and load every scene
//! ```
#![forbid(unsafe_code)]

mod app;
mod console;
mod doc;
mod export;
mod files;
mod graph;
mod inspector;
mod manager;
mod scene_dock;
mod viewport;

use std::path::{Path, PathBuf};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1)
}

/// Compiles all blueprints and loads all scenes; returns the problem count.
fn check(dir: &Path) -> usize {
    let assets = vd_engine::DirAssets::new(dir);
    let mut problems = 0;
    for f in vd_engine::AssetSource::list(&assets) {
        let full = dir.join(&f);
        if f.ends_with(".vbp") {
            match vd_blueprint::Blueprint::load(&full)
                .map_err(|e| vec![e.to_string()])
                .and_then(|bp| vd_blueprint::compile(&bp).map_err(|errs| errs.iter().map(ToString::to_string).collect()))
            {
                Ok(_) => println!("  ✔ {f}"),
                Err(errs) => {
                    problems += errs.len();
                    for e in errs {
                        println!("  ✖ {f}: {e}");
                    }
                }
            }
        } else if f.ends_with(".vscn") {
            match vd_scene::SceneFile::load(&full) {
                Ok(_) => println!("  ✔ {f}"),
                Err(e) => {
                    problems += 1;
                    println!("  ✖ {f}: {e}");
                }
            }
        }
    }
    problems
}

fn main() {
    vd_egui::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("new") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| fail("usage: vibedot new <dir> [--name N] [--template neon|empty]"));
            let name = flag(&args, "--name")
                .unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "My Game".into()));
            let template = match flag(&args, "--template").as_deref() {
                Some("empty") => manager::Template::Empty,
                _ => manager::Template::NeonDrift,
            };
            manager::create_project(&dir, &name, template).unwrap_or_else(|e| fail(e));
            println!("Created '{name}' in {}", dir.display());
        }
        Some("run") => {
            let dir = args.get(1).cloned().unwrap_or_else(|| ".".into());
            let player = export::find_player(flag(&args, "--player").as_deref().map(Path::new), export::Platform::current())
                .unwrap_or_else(|e| fail(e));
            let status = std::process::Command::new(player).arg(dir).status().unwrap_or_else(|e| fail(e));
            std::process::exit(status.code().unwrap_or(1));
        }
        Some("pack") => {
            let (Some(dir), Some(out)) = (args.get(1), args.get(2)) else { fail("usage: vibedot pack <project-dir> <out.vpk>") };
            let n = export::pack_project(Path::new(dir), Path::new(out)).unwrap_or_else(|e| fail(e));
            println!("Packed {n} bytes into {out}");
        }
        Some("export") => {
            let dir =
                args.get(1).map(PathBuf::from).unwrap_or_else(|| fail("usage: vibedot export <project-dir> [--out dist] [--platform p]"));
            let out = flag(&args, "--out").map_or_else(|| PathBuf::from("dist"), PathBuf::from);
            let platform = flag(&args, "--platform").map_or(export::Platform::current(), |p| {
                export::Platform::parse(&p).unwrap_or_else(|| fail(format!("unknown platform {p}")))
            });
            let player = flag(&args, "--player").map(PathBuf::from);
            let problems = check(&dir);
            if problems > 0 {
                fail(format!("{problems} problem(s) - fix them before exporting"));
            }
            let r = export::export_project(&dir, &out, platform, player.as_deref()).unwrap_or_else(|e| fail(e));
            println!("Exported {} ({} KiB pack)", r.app.display(), r.pack_bytes / 1024);
        }
        Some("check") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            let n = check(&dir);
            if n > 0 {
                fail(format!("{n} problem(s)"));
            }
            println!("All good.");
        }
        Some("--help") | Some("-h") | Some("help") => {
            println!(
                "{}",
                include_str!("main.rs").lines().skip(2).take(9).map(|l| l.trim_start_matches("//! ")).collect::<Vec<_>>().join("\n")
            );
        }
        other => {
            let open = other.map(PathBuf::from).filter(|p| p.join(vd_engine::PROJECT_FILE).is_file());
            let options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("Vibedot")
                    .with_inner_size([1480.0, 900.0])
                    .with_min_inner_size([960.0, 600.0]),
                ..Default::default()
            };
            if let Err(e) = eframe::run_native("Vibedot", options, Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, open))))) {
                fail(e);
            }
        }
    }
}
