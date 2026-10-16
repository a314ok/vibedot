#!/usr/bin/env bash
# Vibedot build script.
#
#   ./build.sh                     build the editor (release) and open it
#   ./build.sh editor [project]    same, optionally opening a project
#   ./build.sh run [project]       run a game in the player (default: Neon Drift)
#   ./build.sh release             build editor + player in release mode
#   ./build.sh test                fmt check + clippy -D warnings + all tests
#   ./build.sh export <project> [--platform linux|windows|macos] [--target <triple>]
#                                  package a game as a standalone app in dist/
#   ./build.sh sdk [--target <triple>]
#                                  package the editor itself as an app in dist/
#   ./build.sh clean               remove build output and dist/
#
# Output:
#   dist/<platform>/<Game>/         Linux folder (+ .desktop, icon) and <game>-linux.tar.gz
#   dist/<platform>/<Game>.app      macOS bundle and <game>-macos.zip
#   dist/<platform>/<Game>/         Windows folder (<Game>.exe + game.vpk) and <game>-windows.zip
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS="$ROOT/vibedot"
DIST="$ROOT/dist"

c_bold=$'\e[1m'; c_vio=$'\e[38;5;141m'; c_cyan=$'\e[38;5;80m'; c_red=$'\e[31m'; c_off=$'\e[0m'
[[ -t 1 ]] || { c_bold=; c_vio=; c_cyan=; c_red=; c_off=; }
say()  { printf '%s♦ %s%s\n' "$c_vio$c_bold" "$*" "$c_off"; }
info() { printf '  %s%s%s\n' "$c_cyan" "$*" "$c_off"; }
die()  { printf '%serror:%s %s\n' "$c_red$c_bold" "$c_off" "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo not found - install Rust from https://rustup.rs"

host_platform() {
  case "$(uname -s)" in
    Linux*) echo linux ;;
    Darwin*) echo macos ;;
    MINGW*|MSYS*|CYGWIN*) echo windows ;;
    *) echo linux ;;
  esac
}

platform_of_target() {
  case "$1" in
    *windows*) echo windows ;;
    *apple*) echo macos ;;
    *) echo linux ;;
  esac
}

# Path as the native OS sees it (Git Bash on Windows uses /c/... paths).
native() { if command -v cygpath >/dev/null; then cygpath -m "$1"; else echo "$1"; fi; }

exe_suffix() { [[ "$1" == windows ]] && echo .exe || true; }

# cargo_build <profile> <target-or-empty> <packages...>; prints the output dir.
cargo_build() {
  local profile="$1" target="$2"; shift 2
  local args=(build --profile "$profile")
  for p in "$@"; do args+=(-p "$p"); done
  [[ -n "$target" ]] && args+=(--target "$target")
  (cd "$WS" && cargo "${args[@]}") >&2
  local dir="$WS/target"
  [[ -n "$target" ]] && dir="$dir/$target"
  [[ "$profile" == dev ]] && echo "$dir/debug" || echo "$dir/$profile"
}

archive() { # archive <platform> <dir-containing-app> <app-name> <archive-base>
  local plat="$1" dir="$2" app="$3" base="$4"
  (
    cd "$dir"
    case "$plat" in
      linux) tar -czf "$base-linux.tar.gz" "$app" && info "→ $dir/$base-linux.tar.gz" ;;
      macos|windows)
        if command -v ditto >/dev/null && [[ "$plat" == macos ]]; then
          ditto -c -k --keepParent "$app" "$base-$plat.zip"
        elif command -v zip >/dev/null; then
          zip -qr "$base-$plat.zip" "$app"
        elif command -v 7z >/dev/null; then
          7z a -bd "$base-$plat.zip" "$app" >/dev/null
        else
          info "(zip not installed - skipping archive)"; return 0
        fi
        info "→ $dir/$base-$plat.zip" ;;
    esac
  )
}

cmd_release() {
  say "Building editor + player (release)"
  cargo_build release "" vd_editor vd_player >/dev/null
  info "→ $WS/target/release/vibedot"
  info "→ $WS/target/release/vibedot-player"
}

cmd_editor() {
  local out; out="$(cargo_build release "" vd_editor vd_player)"
  say "Opening the editor"
  exec "$out/vibedot$(exe_suffix "$(host_platform)")" "$@"
}

cmd_run() {
  local project="${1:-$WS/examples/neon_drift}"
  [[ -f "$project/project.vibedot" ]] || die "$project is not a Vibedot project (no project.vibedot)"
  local out; out="$(cargo_build release "" vd_player)"
  say "Running $(basename "$project")"
  exec "$out/vibedot-player$(exe_suffix "$(host_platform)")" "$(native "$project")"
}

cmd_test() {
  say "rustfmt";  (cd "$WS" && cargo fmt --all --check)
  say "clippy";   (cd "$WS" && cargo clippy --workspace --all-targets -- -D warnings)
  say "tests";    (cd "$WS" && cargo test --workspace)
  say "All checks passed"
}

cmd_export() {
  local project="" platform="" target=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --platform) platform="$2"; shift 2 ;;
      --target) target="$2"; shift 2 ;;
      -*) die "unknown option $1" ;;
      *) project="$1"; shift ;;
    esac
  done
  [[ -n "$project" ]] || die "usage: ./build.sh export <project> [--platform p] [--target triple]"
  [[ -f "$project/project.vibedot" ]] || die "$project is not a Vibedot project"
  project="$(cd "$project" && pwd)"
  if [[ -z "$platform" ]]; then
    platform="$([[ -n "$target" ]] && platform_of_target "$target" || host_platform)"
  fi
  if [[ "$platform" != "$(host_platform)" && -z "$target" ]]; then
    die "exporting for $platform from $(host_platform) needs --target <triple> (e.g. x86_64-pc-windows-gnu) and that Rust target installed"
  fi

  say "Building the player for $platform (dist profile: fat LTO)"
  local tool player_dir
  tool="$(cargo_build release "" vd_editor)"
  player_dir="$(cargo_build dist "$target" vd_player)"
  local player="$player_dir/vibedot-player$(exe_suffix "$platform")"
  [[ -f "$player" ]] || die "player binary missing: $player"
  if command -v strip >/dev/null && [[ "$platform" == "$(host_platform)" && "$platform" != windows ]]; then
    strip "$player" 2>/dev/null || true
  fi

  say "Exporting $(basename "$project")"
  mkdir -p "$DIST/$platform"
  local log; log="$("$tool/vibedot$(exe_suffix "$(host_platform)")" export "$(native "$project")" --out "$(native "$DIST/$platform")" --platform "$platform" --player "$(native "$player")")"
  echo "$log" | sed 's/^/  /'
  local app; app="$(echo "$log" | sed -n 's/^Exported \(.*\) ([0-9]* KiB pack)$/\1/p')"
  [[ -n "$app" ]] || die "export failed"
  app="$(command -v cygpath >/dev/null && cygpath -u "$app" || echo "$app")"
  info "→ $app"
  local base; base="$(basename "$app" .app | tr '[:upper:] ' '[:lower:]-')"
  archive "$platform" "$(dirname "$app")" "$(basename "$app")" "$base"
  say "Done"
}

cmd_sdk() {
  local target=""
  [[ "${1:-}" == --target ]] && target="$2"
  local platform; platform="$([[ -n "$target" ]] && platform_of_target "$target" || host_platform)"
  say "Building Vibedot editor + player for $platform (dist profile)"
  local out; out="$(cargo_build dist "$target" vd_editor vd_player)"
  local sfx; sfx="$(exe_suffix "$platform")"
  local dest="$DIST/sdk-$platform"
  rm -rf "$dest"; mkdir -p "$dest"
  local app bin res
  if [[ "$platform" == macos ]]; then
    app="$dest/Vibedot.app"; bin="$app/Contents/MacOS"; res="$app/Contents/Resources"
    mkdir -p "$bin" "$res"
    cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Vibedot</string>
  <key>CFBundleDisplayName</key><string>Vibedot</string>
  <key>CFBundleIdentifier</key><string>dev.vibedot.editor</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>vibedot</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
  else
    app="$dest/Vibedot"; bin="$app"; res="$app"
    mkdir -p "$app"
  fi
  cp "$out/vibedot$sfx" "$out/vibedot-player$sfx" "$bin/"
  cp -r "$WS/examples" "$res/examples"
  if [[ "$platform" == linux ]]; then
    cat > "$app/vibedot.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Vibedot
Comment=Rust 2D game engine with Blueprints
Exec=sh -c 'cd "$(dirname "%k")" && ./vibedot'
Terminal=false
Categories=Development;Game;
EOF
  fi
  info "→ $app"
  archive "$platform" "$dest" "$(basename "$app")" vibedot
  say "Done"
}

cmd_clean() {
  say "Cleaning"
  (cd "$WS" && cargo clean)
  rm -rf "$DIST"
}

case "${1:-editor}" in
  editor) shift || true; cmd_editor "$@" ;;
  run) shift; cmd_run "$@" ;;
  release|build) cmd_release ;;
  test|check) cmd_test ;;
  export|package) shift; cmd_export "$@" ;;
  sdk) shift; cmd_sdk "$@" ;;
  clean) cmd_clean ;;
  -h|--help|help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) die "unknown command '$1' (try ./build.sh help)" ;;
esac
