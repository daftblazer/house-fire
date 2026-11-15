# Open problems

- **The bones' rest path** (state flag bit 2, 80158C84..80158D98): written from the code, never
  met on the home replay; check on the club replay.
- **The scalar channels** (node words 5, 6; 801BD7BC): never read on the home replay; not built.
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
