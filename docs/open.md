# Open problems

- **The bones' rest path** (state flag bit 2, 80158C84..80158D98): written from the code, never
  met on the home replay; check on the club replay.
- **The scalar channels** (node words 5, 6; 801BD7BC): never read on the home replay; not built. The club's laser light needs them.
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
- **Lamps' "on" animations** (the club): which animation an object poses beyond its idle one
  follows its state (lit or not); 11 of the club's 15 lamp colours wait on it, and the laser's
  reach and place (1,258 of 1,386 grid cells from the engine alone).
- **The picture's state not recorded** (home-a36's ';' records have the context's state and
  the pixel engine's registers as ERC_ApplyState left them): fog, and GX calls a draw method
  makes after ERC_ApplyState (the shadow mask's constant alpha?).
- **Log mode's frame at 97.47% within 4** (frame 2400, after F1: floors 97.33%, shadows
  97.84%, rigid 97.99%): floors 5,487 px off (trilinear LOD, the porch near black in the
  reference, birch leaves 1,091 px darker with no shadow draw after them: unmeasured). The
  terrain's second layer is the terrain shader's own map (PTTEXMTX0 from `Shader::position_uv`).
- **The engine mode's frame at 97.21% whole** (render-check HF_ENGINE=1 --card, frame 2400,
  after E1: rigid 97.02%, floors 97.35%, shadows 95.90%; log mode 97.47%). Stuck after two
  packets each (W1, E1): the shadows' shears (SHADOW_SHEAR_OBJECT 1.144, _WALL 0.9286 fitted at
  one clock; the 75 indoor objects' unsheared; ruled out: nearest/strongest light, sums of the
  four directions by rank, reach, colour or distance, per-slot mixes; draws on one light set
  share one shear; no other log has draw records at another clock) and the money trunk 678 /
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

