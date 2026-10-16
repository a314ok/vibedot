# Vibedot

This repository contains two engines side by side:

| Directory | What it is |
|-----------|------------|
| [`godot/`](godot/) | The original **Godot Engine** (C++), unchanged, kept as the reference implementation. Build it with SCons from inside `godot/`, as usual. |
| [`vibedot/`](vibedot/) | **Vibedot**, a new 2D engine written in **Rust**. It follows Godot's architecture and API (SceneTree, Node, Variant, StringName, signals, notifications, pause and process modes). It adds visual scripting (Blueprints), an HDR 2D renderer on wgpu and a redesigned editor. |

## Vibedot in one minute

```bash
./build.sh                                     # build and open the editor
./build.sh run                                 # play the Neon Drift demo
./build.sh export vibedot/examples/neon_drift  # standalone app in dist/
./build.sh sdk                                 # the editor packaged as an app
./build.sh test                                # fmt + clippy + tests
```

On Windows, use `.\build.ps1`. For architecture, the Godot to Vibedot API mapping and
the scope, see [`vibedot/README.md`](vibedot/README.md).
