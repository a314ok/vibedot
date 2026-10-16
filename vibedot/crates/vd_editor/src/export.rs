//! Exporting a project as a standalone app: the player binary plus the
//! project packed into `game.vpk`, laid out per platform:
//!
//! * Linux:   `<Name>/<name>` + `game.vpk` + `<name>.desktop` + `icon.png`
//! * Windows: `<Name>/<Name>.exe` + `game.vpk`
//! * macOS:   `<Name>.app/Contents/{MacOS/<name>, Resources/game.vpk, Info.plist}`

use std::path::{Path, PathBuf};

use vd_engine::{DirAssets, ProjectSettings, pack_source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    MacOs,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "linux" => Some(Platform::Linux),
            "windows" | "win" => Some(Platform::Windows),
            "macos" | "mac" | "osx" => Some(Platform::MacOs),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("project error: {0}")]
    Project(String),
    #[error("player binary not found (looked for {0}); build it with ./build.sh")]
    NoPlayer(String),
}

/// Lowercase, filesystem-safe name.
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_owned();
    if s.is_empty() { "game".into() } else { s }
}

/// Locates the player executable: explicit path, `VIBEDOT_PLAYER`, or next
/// to the running editor.
pub fn find_player(explicit: Option<&Path>, platform: Platform) -> Result<PathBuf, ExportError> {
    let file = if platform == Platform::Windows { "vibedot-player.exe" } else { "vibedot-player" };
    let mut candidates = Vec::new();
    if let Some(p) = explicit {
        candidates.push(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("VIBEDOT_PLAYER") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(file));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| ExportError::NoPlayer(file.to_owned()))
}

/// Writes `game.vpk` for the project.
pub fn pack_project(project_dir: &Path, out: &Path) -> Result<usize, ExportError> {
    let src = DirAssets::new(project_dir);
    let bytes = pack_source(&src);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, &bytes)?;
    Ok(bytes.len())
}

fn info_plist(name: &str, exe: &str, id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleExecutable</key><string>{exe}</string>
    <key>CFBundleIdentifier</key><string>{id}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

fn copy_executable(from: &Path, to: &Path) -> Result<(), ExportError> {
    std::fs::copy(from, to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(to)?.permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(to, perm)?;
    }
    Ok(())
}

/// Simple generated app icon (accent gradient diamond), so exported apps
/// have something nicer than a blank icon.
pub fn generate_icon(size: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(size, size);
    let c = size as f32 / 2.0;
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (dx, dy) = ((x as f32 + 0.5 - c) / c, (y as f32 + 0.5 - c) / c);
        let d = dx.abs() + dy.abs();
        let t = (dx + 1.0) * 0.5;
        let (r, g, b) = (139.0 + (34.0 - 139.0) * t, 92.0 + (211.0 - 92.0) * t, 246.0 + (238.0 - 246.0) * t);
        let a = ((0.92 - d) * size as f32 * 0.5).clamp(0.0, 1.0);
        let bg = ((1.0 - (dx * dx + dy * dy).sqrt()) * size as f32 * 0.5).clamp(0.0, 1.0);
        let mix = |fg: f32, bgc: f32| (fg * a + bgc * (1.0 - a)) as u8;
        *p = image::Rgba([mix(r, 14.0), mix(g, 14.0), mix(b, 24.0), (bg.max(a) * 255.0) as u8]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png encoding");
    out
}

#[derive(Clone, Debug)]
pub struct ExportResult {
    /// The app folder / bundle.
    pub app: PathBuf,
    pub pack_bytes: usize,
}

/// Exports `project_dir` into `out_dir` for `platform`.
pub fn export_project(
    project_dir: &Path,
    out_dir: &Path,
    platform: Platform,
    player: Option<&Path>,
) -> Result<ExportResult, ExportError> {
    let settings = ProjectSettings::load_dir(project_dir).map_err(|e| ExportError::Project(e.to_string()))?;
    let name = settings.name.clone();
    let exe_slug = slug(&name);
    let player = find_player(player, platform)?;
    let icon = generate_icon(256);
    let (app, pack_path) = match platform {
        Platform::Linux => {
            let app = out_dir.join(&name);
            std::fs::create_dir_all(&app)?;
            copy_executable(&player, &app.join(&exe_slug))?;
            std::fs::write(app.join("icon.png"), &icon)?;
            std::fs::write(
                app.join(format!("{exe_slug}.desktop")),
                format!(
                    "[Desktop Entry]\nType=Application\nName={name}\nExec=sh -c 'cd \"$(dirname \"%k\")\" && ./{exe_slug}'\nIcon=icon\nTerminal=false\nCategories=Game;\n"
                ),
            )?;
            (app.clone(), app.join("game.vpk"))
        }
        Platform::Windows => {
            let app = out_dir.join(&name);
            std::fs::create_dir_all(&app)?;
            copy_executable(&player, &app.join(format!("{name}.exe")))?;
            (app.clone(), app.join("game.vpk"))
        }
        Platform::MacOs => {
            let app = out_dir.join(format!("{name}.app"));
            let contents = app.join("Contents");
            std::fs::create_dir_all(contents.join("MacOS"))?;
            std::fs::create_dir_all(contents.join("Resources"))?;
            copy_executable(&player, &contents.join("MacOS").join(&exe_slug))?;
            std::fs::write(contents.join("Resources/icon.png"), &icon)?;
            std::fs::write(contents.join("Info.plist"), info_plist(&name, &exe_slug, &format!("dev.vibedot.{exe_slug}")))?;
            (app.clone(), contents.join("Resources/game.vpk"))
        }
    };
    let pack_bytes = pack_project(project_dir, &pack_path)?;
    Ok(ExportResult { app, pack_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vd_engine::AssetSource;

    #[test]
    fn slugs() {
        assert_eq!(slug("Neon Drift!"), "neon-drift");
        assert_eq!(slug("   "), "game");
    }

    #[test]
    fn export_layouts() {
        let tmp = std::env::temp_dir().join(format!("vd_export_test_{}", std::process::id()));
        let project = tmp.join("proj");
        vd_engine::templates::write_to(&project, &vd_engine::templates::empty_project("My Game")).unwrap();
        let fake_player = tmp.join("vibedot-player");
        std::fs::write(&fake_player, b"#!/bin/sh\n").unwrap();
        for platform in [Platform::Linux, Platform::Windows, Platform::MacOs] {
            let out = tmp.join(format!("out_{platform:?}"));
            let r = export_project(&project, &out, platform, Some(&fake_player)).unwrap();
            assert!(r.pack_bytes > 100);
            let pack = match platform {
                Platform::MacOs => r.app.join("Contents/Resources/game.vpk"),
                _ => r.app.join("game.vpk"),
            };
            let pack = vd_engine::PackAssets::open(&pack).unwrap();
            pack.verify().unwrap();
            assert!(pack.list().contains(&"scenes/main.vscn".to_owned()));
        }
        assert!(tmp.join("out_MacOs/My Game.app/Contents/Info.plist").is_file());
        assert!(tmp.join("out_Linux/My Game/my-game").is_file());
        assert!(tmp.join("out_Windows/My Game/My Game.exe").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
