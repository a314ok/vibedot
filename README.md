# Vibedot

This repository contains two engines side by side:

| Directory | What it is |
|-----------|------------|
| [`godot/`](godot/) | The original **Godot Engine** (C++), unchanged - the reference implementation. Build it with SCons as usual from inside `godot/`. |
| [`vibedot/`](vibedot/) | **Vibedot** - a new engine written in **Rust**, following Godot's architecture and API (SceneTree, Node, Variant, StringName, signals, servers) with visual scripting (Blueprints), an HDR 2D renderer and a redesigned editor. |

See [`vibedot/README.md`](vibedot/README.md) for building and using the Rust engine.
