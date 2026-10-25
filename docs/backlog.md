# NeoBrawl backlog (not yet implemented)

Collected from design talks in the side session, 2026-10-26.

## Feel and mechanics
- **Charge curve by threat.** Replace the linear slowdown (`937ccde`) with a curve over range gained, not time held.
  - Free jab zone up to the charge that reaches ~4 units in under ~0.25 s, the human reaction time: about 25 ticks, ~28% of a full charge. Derive it from drag and power, not a fixed percentage.
  - Beyond that: speed = 1 − (1 − floor)·smoothstep(e), where e is the extra range past jab range, normalised to 0..1. Floor ≈ 0.45.
  - Sound and motion tell when crossing the jab threshold.
  - Charge fills fast early.
  - Tune with AI-vs-AI sweeps, aiming for kills split ~evenly between jabs and loaded throws.
- **Dash while charging:** full distance, but bleeds about a third of the charge.
- **Bouncing boxes:** a glancing wall hit bounces; only a near-perpendicular hit (high speed into the wall) shatters.
  - Speed loss per bounce; harmless after 2–3 bounces.
  - A spark on each bounce.
  - Box-on-box and box-on-player still explode.
- **Single-player camera:**
  - Anchored on the player, leaning toward the cursor (Gungeon-style).
  - Weighted points of interest, zoom limits with delay in both directions, a dead zone, critically damped springs.
  - Off-screen enemy edge marks; brief moment framing on kills.
- **Bot pathfinding:** the heuristic bot only steers locally and gets stuck behind maze walls.

## Multiplayer
- Shareable room links, WebRTC peer-to-peer, a tiny signalling server.
- Rollback netcode with 2–3 ticks of input delay.
- Late remote throws drawn from the thrower's hand and caught up visually.

## Learning bots
- **Headless match runner and recorder.** A deterministic match is just its seed plus the inputs. Bot-vs-bot at scale, plus recorded human games.
- **Observations:**
  - Own state.
  - Enemy, boxes and projectiles as entity sets.
  - 32 rays reporting distance and hit type.
  - All delayed by reaction time.
- **Actions:**
  - 8-way movement, charge, dash.
  - Target-relative aim: pick the enemy, a box, or the enemy mirrored across a nearby wall (bank shot), then a continuous offset and a lead of 0–1 s.
  - A free-aim fallback.
  - A motor model: capped turn rate with slight overshoot.
- **Model:** ~0.5M parameters (entity encoder plus a small memory layer), runnable in WASM.
- **Training:**
  - Imitation warm start from the heuristic bot, with a decaying pull back toward that style.
  - Then PPO self-play against a league (past versions, heuristic levels, exploiters). Reward winning only.
- **Human handicaps:** ~200 ms reaction, an actions-per-second cap, aim noise.
- **Elo ladder** plus a replay theater that picks close, high-rated matches.

## Anthill (prototypes/anthill)
- Bring the box-carrying ants (`boxes.html`) into the realm; check long-run clustering.
- A playable ant level: red defends, blue players raid. A GPU ant sim for thousands.
- The breeder's taste learning and the GPU batch solver, kept as tools.
