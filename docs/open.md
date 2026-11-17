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
- **Floors 85.5% and shadows 86.6% within 4** (frame 2400, R2a/R2a2, left over from stage 5b;
  against the frames with gcn-recomp's constant destination alpha):
  terrain strips ~24,000 px off (second layer through GX_TEXMTX0 then the dual-texture matrix
  0x40, which ERC_ApplyState loads natively from the context's +0x6A4: not in the log, so
  TERRAIN_COLOUR_* is fitted; next step: add +0x674 and +0x6A4 to the ';' record), trilinear LOD;
  `shadow_of` (flat black, alpha 127) fitted; the porch near black in the reference;
  birch leaves 1,091 px darker in the reference with no shadow draw after them.
- **The engine mode's frame at 86.64% within 4, not R1's 91.82%** (render-check HF_ENGINE=1
  --card, frame 2400, stage 5c; log mode 97.63%; with the log's light sets 89.18%). Left, by
  pixels beyond: windows 2,394 (the frame lines lose the depth to the wall where the original's
  win; matrices, meshes and materials equal the log's, order ruled out); walls, window walls,
  diagonals and fence ~4,700 (14 of 35 wall light sets differ by one light); the money trunk
  678 and the cheat gnome 78 drawn but not by the original (flags, fields, selectors, ResData,
  lists, level ruled out; the draw records' 72 bounds bytes undecoded); walnut door 1,009,
  outer lot 723, lattice door 571, aquarium 207. Still from the log in that mode: the camera,
  the lot choice, the clock passes. Draw list (LOCKSTEP_FRAME_DRAWS=1039): 570 same, 3
  DIFFERENT, 26 missing (objects' script animations, sink counter variants, wall caps).
- **The engine mode's Sims and shadows** (W1, stage 5d): the Sims' skinned parts, their 34
  shadow draws and the portrait still come from the log (1,551 px, 0.54% of the frame): the
  controller running the Sims' layers to a tick's pose from a loaded lot, and the run-time
  skin/clothes texture, are not built. Shadows from the engine at 78.55% (log mode 86.59%):
  shears fitted at one clock (SHADOW_SHEAR_OBJECT 1.144, _WALL 0.9286, source unknown), the
  indoor objects' (from their light sets) not made (75 draws unsheared), the diagonal walls'
  extends forms. The camera from the log's 'c' parameters (no input on a loaded lot). The front
  wall's foot shows the floor where the reference is dark (the `_garbage` pieces not drawn).

