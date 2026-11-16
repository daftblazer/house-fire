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
