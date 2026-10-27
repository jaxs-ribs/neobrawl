# NeoBrawl backlog (not yet implemented)

Collected from design talks in the side session, 2026-10-26.

## Feel and mechanics
- **Single-player camera:**
  - Anchored on the player, leaning toward the cursor (Gungeon-style).
  - Weighted points of interest, zoom limits with delay in both directions, a dead zone, critically damped springs.
  - Off-screen enemy edge marks; brief moment framing on kills.
- **Bot pathfinding:** the heuristic bot only steers locally and gets stuck behind maze walls.

## Multiplayer
- Shipped: invite links, WebRTC via PeerJS, rollback, correction easing, rematch.
- Own signalling/TURN server if PeerJS's free cloud proves flaky.

## Bots
- Teach the bot to jab: it never throws inside jab range (0-2% of throws), though jabs land as often as loaded throws. Tried stepping in and winding up late; it made the bot catch dodgers too often, so it was reverted.

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
- Shipped: the anthill mode (workers, soldiers, stackers, overflow, shields). Next:
  - More ant kinds (a guard that holds the hill, a runner that steals your box).
  - Workers need pathing once inner walls stay in (the mode clears them today).
  - Tune with real play: idle for 90 s, the colony banks about half the floor.
- A GPU ant sim for thousands.
- The breeder's taste learning and the GPU batch solver, kept as tools.
