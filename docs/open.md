# Open problems

## Hardest first (ranked 2026-11-17, master e290d68; for the strongest model, not for packets)

Ranked by how much hangs on each and how little a fitted answer helps. Each needs reverse
engineering of the original's code, not more fitting. The list below this section is unchanged.

1. **The simulation run free.** Nothing runs without a log yet: `replay` follows the log step by
   step and sets every native difference back to the log's (replay.rs, 6,153 lines, against
   simulator.rs 288). The 0-difference figures therefore say "each step agrees given the log's
   state before it", not "the engine plays the game". What the log still supplies (home / club
   replay): primitives answered from the log 24,516 / 31,648 (Animate 44: 4,032 / 27,666;
   CompleteGoal 56: native since 2026-11-17, primitives.md; IdleForInput 17: native since 2026-11-17; Relationship2 26: native since 2026-11-17; Idle 0 (both forms, the lockout) and NotifyStackObject 49: native since 2026-11-17; 22: native since 2026-11-19; change suit 6: 13 / 70; 12, 24, 41 on home), locals set to the log's 20,615 / 12,491,
   parameters 423 / 110, places with no native walk 18 / 18 (the tick-0 placements native since 2026-11-19), route runs not modelled 1 / 26,
   routing slots from a stack variable (native since 2026-11-19), the club's autonomy candidates different 82,
   trees not found on the disc 13 / 82 steps (367, 8217, 8233, 8235, 8236, 8312, 8330), the
   household and later-made objects not run, the funds, the random numbers. Longest run of
   steps with nothing set: home 50,938. First: a free mode (the log only as the player's
   commands, dialog answers and random seed) that reports the first tick and step where the
   native stack leaves the log's; then that tick driven to the log's end, on home then club.
   The measure meanwhile (2026-11-17): `tools/log_debt.py` on a replay's summary, to drive to
   zero (zero = the log only compared with). After CompleteGoal went native (the save's NGBH
   marks; 20,153 / 2,772 uses, all the log's): home.log 5,519, club.log 29,895 (Animate 44 the
   bulk of both), home-full 1,556, club-full 3,095. The largest kinds now: home-full the
   frames' parameters 469, Relationship2 26 107, places with no native walk 224, scope 8 152,
   scope 18 116; club-full Animate 511 (470 with no archive animation, 1 different: the preload set by the lot's house number and the held frame's speed, 2026-11-17), 22 189,
   26 146, scope 18 252, routing slots 200. IdleForInput (17) native since as well (primitives.md):
   home-full 1,511, club-full 2,295. Relationship2 (26) native since: home.log 5,362, club.log
   28,937, home-full 1,399, club-full 2,137. `replay`'s "primitives by opcode"
   lines rank the primitives. The frames' uncleared locals are not owed (loop.md 5e e3).
   **State 2026-11-17 (evening): home-full 528, club-full 778** (home.log 4,553, club.log
   28,443). `replay --free | tools/log_debt.py` gives each kind's first tick: the run is free
   until tick 1 on both logs, so the list is by what comes first, not by size. Native or
   explained since the lines above: Relationship 24, DistanceTo 11, DirectionTo 12; ChangeSuit
   6, SetBalloon 41, MakeActionString 50 (the presentation's); person data 27 (the person's
   turns); the hide field (data 50) cleared before a check; Animate's global table (Global's
   STR# 128) and its event's value in the frame; the neighbourhood's data (scope 34) from the
   save; write-only lookups (the observer logs 0 for them); a step whose tree record is
   another tree's. **Left over, by kind (home / club):**
   - places with no native walk 18 / 18 (jumps to a tile's centre, walks' ends), route
     runs not modelled 1 / 26: the routing work left (item 3; LookTowards native since
     2026-11-19);
   - frames' parameters 46 / 2: 9 are stacks saved at an Idle of 15000 or 20000 whose count is
     0 at their first step (other saved Idles keep theirs: something ends those sleeps at the
     load, as NotifyStackObject would; zeroing every saved Idle's count made 41 more differ);
     the rest are calls with no parameters given whose temps differ (downstream of the others);
   - the club's ad range (scope 12) 66: the dance floor's first tile reads datum 15 as 70 at
     its 0x1019 node 7 in the rounds after a dancer starts dancing again, where the engine has
     its own 50. Tried and dropped: not restoring the range after an evaluation's guard (no
     change), the delivered interaction's advertisement made the range in IdleForInput (9 more
     differ: the dancers' own reads at tick 2 are 50);
   - the driven run's first partings (simulation.md, end of "The driven run"): club 164's first
     Animate true at once in the original (TryAnimate's early answer, unread); home the scooter's
     Allow Intersection (OBJf 5) pushed by the route follower for a stuck walker (unread);
   - IdleForInput's +0x240 test (`fn_80225D98`, read 2026-11-19): the engine has only its
     queue part ("something queued"); the priorities (entry +40 against the person's +74), the
     entry's flag 2, the +150 and +1100 fields and the motives below -99 are not modelled;
   - person data 22, 23, 32, 33 at the club 24 (the routing states' and the load's), 27 for
     one dancer (178) 6;
   - Budget 25 9 (the family's funds are not read from the save), TestInteractingWith 37 6
     (the person's list at +0x49C), GenericSimCall 1 3 / 3, FindGoodLocation 16 1;
   - the motives after tick 1 (the paper girl's hunger) 1 / 1, engine frames over an Idle 7,
     TestObjectType 5, the expression 10 / 1, Animate 5 / 1 different.
   **Started 2026-11-18: the engine's own tick as the driver** (`replay --drive`, simulation.md "The
   driven run"): home-full runs the native load pass and tick 1 the log's and parts at tick 2 (the
   engine's frame over 251's Idle); club-full (`--hood` the session's card) runs the load and tick
   1 the log's and parts at tick 2 (164's 281:2; the motive copy and the
   visitors' mark before it are native, their tests inferred: simulation.md "The driven run").
2. **The original's loop of frames and ticks**: THE RULE FOUND 2026-11-17 (simulation.md
   "Frames and ticks"; `routing::FrameClock`): a clock in milliseconds, 33 a frame times the
   speed's factor, a tick for every 42.0 (802CEBFC), held to 840, afresh after the speed's
   change; native to the bit on every tick of the home session and all but one of the club's.
   The replay's animation frames are placed by it (home 8,598 of 8,598, club all but the 42
   beside menu steps; loop.md 5e h). Left, each now a matter of using it: the 20 walking places a frame early or late (plan.md "Next"); the club's laser
   place after tick 6; the male's head a tick behind; which bone set a skinned draw uses (296
   of 4,712); which loads pause whose Animate (primitives.md 412-418). The held frame (club
   tick 3204, 2026-11-17) is the player's input: a speed change made in a frame that runs no
   tick (the clock at 3 ms, 36 after the frame) holds that frame (no tick, the clock afresh)
   and plays it at the speed before; one made in a tick's frame (1010, 1236, 3459, 5726) does
   not. Which frame the press came in is the input (the log's count); the held frame's speed is
   native (`AnimReplay::held`): club Animate different 41 -> 1. Unread: the frame function's own order of
   tick, animation and draw (fn_8015F4B4 and its callers 800851A4, 80064794).
3. **The planner and the walk, natively and whole.** The pieces agree where the log is between
   them; free, the walk parts at six frames and stays off (the door's fast mode, a ramp, two
   brakes, a walk begun at speed: simulation.md 940). Fitted or unread: the planner's weight 3
   (2.5 to 3.5 fit, simulation.md 854), the ground's own cost (1160), a link the original does
   not make (1294), plans' second searches (1244), 34 footprints not found (1583), walls'
   corners (787). Club: the planning step not the log's 8 times, obstacles not all the
   planner's 4. Item 1 cannot pass a walk without this.
4. **The Sims' root turn and the tracks not made** (club Sims' own pixels 60.01%, home 92.50%):
   4 of the club's 8 drawn Sims are turned 2 to 4 eighths from their direction though data 1 is
   the original's at all 84,780 person-ticks, so the bones' root takes its turn from somewhere
   else (the animation's root rotation, or the follower's float angle: unchecked; 1 of 2 tries
   left). With it: walk, turn and idle tracks, the face weights after tick 643.
5. **The shadows' shears from the code, not fitted** (home engine shadows 95.90%, club 86.36%):
   the divisors 0.4 and 0.9744|z|+0.629 and the power 1.7 were fitted on two suns and have
   no source; two packets (W1, E1) stuck on home. Read the original's shadow projection (the
   draw method after ERC_ApplyState) for the matrix it builds from the sun and the light set.
6. **What the original does not draw**: the money trunk (678 px), the gnome (78), the club's robe
   Sim (+0.26%). No screen box, flag, field, selector, ResData, list or level tells them from
   their drawn neighbours; the rule is unknown. Probably one visibility test (room, portal or
   a per-object draw state) not yet seen by the observer: a store watch on the draw list's
   build is the lead.
7. **The club's own render-to-texture**: the world dumps are black there (an EFB copy with clear
   between the scene and the 2D layer, unread), so the club has no clean reference, and the
   dance floor is the biggest single miss (16.9k of 35.6k px). Likely one mechanism.
8. **Fran's description**: her bytes are in no card block and no disc file searched; the story
   mode writes them somewhere not found. The first piece of stage 9 (the story mode) and a
   test of whether the save and neighbourhood formats are really understood.

Not hard, only work (good packets): the light sets' grid lookup (0x80169ABC, 15 meshes), models
as plain arrays, the multiplayer blockers, the simulation speed list, the device run (needs SSH).

## The rest

- **The bones' rest path** (state flag bit 2, 80158C84..80158D98): written from the code, never
  met on the home replay; check on the club replay.
- **One Sim animator speed** of 17,198 (an Animate the run list misses).
- **Display-list ('g') draws** are not tied to model parts yet; 130 strips are followed by several world
  matrices.
- **Which bone set a skinned draw uses** when it is not the tick's (296 of 4,712 matrices are a
  bone set's within 2 ticks): the frame/tick order of drawing and ComputeMatrices.
- **Multiplayer blockers found by the quality pass** (Q1, 2026-11-15): `dialog_object`,
  `dialog_answer` and `stop_pass` are global (one open dialog stops the pass for everyone);
  `queue_player_command` writes into the world with no player or tick; `presentation` is an
  unbounded Vec on the world. To fix when the command layer is built.
- **Simulation speed left** (Q1): `ScriptVariables::get` scans objects for slot and neighbour
  scopes; `Node`'s byte order is branched on at every operand read (decode at load);
  `World::unindex`/`remove` are O(n), `next_id` builds a set per call.
- **Models kept as raw big-endian bytes** (Q2): model.rs holds positions, texture coordinates,
  colours and normals as the disc's bytes, and `slot_bytes` is cloned per batch. For the GPU
  path (phase 5) they become plain vertex and index arrays built once at load (model, draw,
  scene and assets change together).
- **`Stream` and `RotStream` duplicate the window stepping** (Q2): one generic stepper.
- **Frame build left** (P1, 1.28 ms on the PC): the object-part loop ~0.25 ms (animated objects'
  poses), floor strips and meshes rebuilt each frame ~0.2, assemble 0.13, the Sims 0.13;
  `Lighting` cloned per draw; models still raw bytes (only the mesh middle made at load).
- **Study samples from HashMaps** (the RUNS and PLACES studies' "followed tracks" and Sims
  lines) vary run to run: sort them so the outputs are stable.
- **Fran's description** (club-a1.log, Sim 80f94f70, af_*_fran parts): not a lot object nor a
  made Sim; her bytes are in no card block and no disc file searched (quickdat, characte,
  datasets, binaries, levels, neighbourhood files). Probably the story mode's: find where the
  game writes her description.
- **The child part tables** are read but unchecked: no logged Sim is a child.
- **Description bytes 8, 10 and 16-22** (skin, body shape?) not yet understood.
- **The club's laser place after tick 6** (K1): the lamps pose the animation their graphic
  (data field 0) picks, from the engine alone (club: own lights' colours 15 of 15, places 14,
  cells 1,386 of 1,386, sums 57,444 bit for bit); the laser's "fun" plays 1.98 frames per render
  frame and a tick holds one or two render frames, which the simulation doesn't count, so the
  lamps are posed at place 0 (one light's place at the tick-28 rebuild). The drawn objects'
  models (drawlist's `idle_place`) still pose the idle animation, not the graphic's.
- **The picture's state not recorded** (home-a36's ';' records have the context's state and
  the pixel engine's registers as ERC_ApplyState left them): fog, and GX calls a draw method
  makes after ERC_ApplyState (the shadow mask's constant alpha?).
- **Log mode's frame at 97.47% within 4** (frame 2400, after F1: floors 97.33%, shadows
  97.84%, rigid 97.99%): floors 5,487 px off (trilinear LOD, the porch near black in the
  reference, birch leaves 1,091 px darker with no shadow draw after them: unmeasured). The
  terrain's second layer is the terrain shader's own map (PTTEXMTX0 from `Shader::position_uv`).
- **The engine mode's frame at 97.21% whole** (render-check HF_ENGINE=1 --card, frame 2400,
  after E1: rigid 97.02%, floors 97.35%, shadows 95.90%; log mode 97.47%). Stuck after two
  packets each (W1, E1): the shadows' shears (found on the club in K5: walls from the sun over
  |z|+0.4, indoor objects an inverse-power sum of the light set, constants in engine_scene.rs;
  home's engine shadows held at 95.89%: its gap is elsewhere) and the money trunk 678 /
  gnome 78 px drawn but not by the original (no screen box separates the trunk, x 0.0617,
  from the leftmost drawn object, 0.0618; flags, fields, selectors, ResData, lists, level ruled
  out). Also left: the outer lot 619, window glass 226, counter 158, wall caps 127, lattice door
  108, aquarium 105, birch 123; 15 object meshes' light sets (below). Still from the log: the
  camera ('c' parameters), the lot choice, the clock passes.
- **The engine's Sims** (S1, T1, A1): drawn from the engine's poses, textures and headings
  (Sims' own pixels 92.50%, portrait 96.10%; log mode 88.50%; `view`'s shot 99.99%). Bones
  within 1e-4 of the log's: the male 53 of 58, the cook 47 (the rest not drawn: roots, thumbs,
  hair nodes). Left: the male's head (its parts' world matrix) is the engine's of the tick
  before, 0.027 tile off, the cook's is not; tracks other than action, carry and face (walk,
  turn, idle) not made; the face layer's node weights from '=' records that stop at tick 643;
  the carry loop checked at one tick. Still from the log: which Sim the portrait shows, the
  'k' frame places, five held objects (Prop_Spoon, Carry_*: a few pixels). Shadows' shears:
  above.
- **The device run**: the RK3566 run of `view` (aarch64 build, `--ticks 1200 --frame-times`)
  needs the user's SSH; by the PC's figures the CPU's frame build (~18 ms there) is the risk
  (docs/assets.md "The budget").
- **15 object meshes' light sets** (engine mode, L3): the trash can 2, a deckchair 1, the lattice
  door 1, two cacti 3 each, two rose bushes 2 each: the grid point (a tie at x.5: (19.5, 6.5)
  takes x 20, (19.5, 10.5) x 19). The original's lookup (0x80169ABC) rounds a position the caller
  passes (not the place, not a mesh middle) and searches the cell's four room bytes for the
  object's room (a default entry if absent); the engine's `cell` ignores that list.
- **The club's world dumps are black** (club-d3000.log, frames 3000-3001, GCN_DUMP_WORLD): the
  recomp copies the world at the first draw after the 2D layer's marker changes
  (render_vk.cpp, `g_world_taken`), and on the club the EFB is black then: likely an EFB copy
  with clear (the dance floor's or lasers' render-to-texture?) between the scene and the 2D
  layer. Not read. render-check's club reference is the full frame (work/frames-club3: the
  frame_ PNGs copied as world_), so the portrait and HUD count as misses in both modes. (The
  log's two frames in tick 964 were the 'two passes': split since 76f6c37.)
- **The club's Sims after D1** (2026-11-17): the persons' directions (data 1) are now the
  original's at every tick (route followers turn them; turn-only routes from LookTowards;
  `LOCKSTEP_DIRECTIONS=1` on club-d3000d.log: 84,780 person-ticks, 0 different). Club engine mode
  87.59% to 91.08% whole, Sims' own 38.05% to 60.01%, rigid 92.02%, shadows 86.36%; home unchanged.
  Left: the drawn heading is the direction's for 4 of the 8 drawn Sims (178, 172, 169, 164); 177 and
  174 are drawn turned by 4 eighths from their direction, 171 by 3, 57 by 2: the bones' root turn is
  not the direction alone (likely the animation's own root rotation, or the heading taken from the
  follower's float angle, not data 1: unchecked). Route_Run's end-of-primitive write (8022081C) and
  other LookTowards operands not made (not met on the club).
- **The club in engine mode at 87.59% whole** (before D1, now 91.08%: above; K2-K5; frame 3001 of club-d3000b.log, work/frames-club3;
  log mode 97.62%): rigid 88.78%, floors 94.28%, shadows 84.26%, Sims 38.05%. K5: shadow shears
  from the sun and the light set, multi-tile objects lit at the leader's tile, lamps' lights
  at the grid's last build, floor coverings (flat opaque meshes under 0.05: drawn with the
  floors, no shadow). Left (35.6k px): the five Sims ~12k (poses, headings, a black arm on the
  dancer) and the portrait 3k; the dance floor 16.9k; the robe Sim the game doesn't draw
  (+0.26% if skipped, no data rule); bar 1.2k, booth 1.2k, doors 960, counter 0.5k. Lead: the
  grid-point tie (floor in y fixes the doors 960 -> 173 px, home wants ceil: -0.22%).
  Unexplained: the shear divisors (0.4; 0.9744|z|+0.629) and the power 1.7, fitted on two
  suns. Still from the log in that mode (K2): a moving lamp's place ('e' records), the Sims'
  identities (dressing records, by order). Reference is the full frame, so the portrait and
  HUD count as misses in both modes.