# NeoBrawl — browser port

NeoBrawl (Three Lights Left, Unity 5.3.4, Android, 2016) rebuilt outside Unity:
Rust compiled to WebAssembly for the game, WebGPU for rendering.

**Play:** https://jaxs-ribs.github.io/neobrawl/

```
tools/          Unity 5.3 serialized-file reader + extractor (no UnityPy): scene graph,
                sprites, ETC2/EAC textures, meshes, uGUI canvases/images/texts/buttons,
                colliders, rigidbodies, and every script's serialized fields
engine/         Rust crate
  src/physics.rs  2D rigidbody solver standing in for PhysX (OBB/circle, CCD substeps,
                  layer matrix, IgnoreCollision, collision/trigger events, raycasts)
  src/ui.rs       uGUI layout: CanvasScaler, RectTransform anchors, canvas modes, hit tests
  src/game/       line-by-line ports of the MonoBehaviours; coroutines become tasks
  tests/          headless gameplay tests (menu flow, pickup, throw, kill, scoring)
web/            index.html + main.js (WebGPU, touch/keyboard/gamepad, WebAudio) + assets
```

## Controls

- **Touch (phones/tablets):** the original two-player twin-stick layout — each player
  owns two corners: one stick moves, the other aims; hold to charge, release to throw.
- **Keyboard (two players, one laptop):** P1 `WASD` move, `T F G H` aim · P2 arrows move,
  `I J K L` aim. Hold the aim keys to charge, release to throw.
- **Gamepads:** left stick moves, right stick aims; first pad is P1, second is P2.

Online play used Photon Cloud with the 2016 app id and isn't available in the browser;
the one-screen two-player mode, tutorial and menus are complete.

## Build & test

```bash
./build.sh
```

```bash
cargo test --release --manifest-path engine/Cargo.toml
```

URL flags: `?debug` state overlay, `?mute`, `?fresh` (ignore saved settings), `?seed=N`.
`window.__nb.state()` exposes live game state for automation.

## Fidelity notes

- Audio was FMOD FSB5/Vorbis inside Unity's resource file; it was converted once with
  vgmstream and re-encoded as mp3.
- The menu's "intro fights" replayed inputs stored in the developer's PlayerPrefs, so in
  the shipped game (and here) those players stand still.
- Matches end at the first score ≥ 99 that is a multiple of 8 (i.e. 104), as in the original.
