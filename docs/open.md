# Open problems

- **The bones' controller scale and mode word** (stage 4.4, iteration 186): the '<' record does
  not log the controller's +84 (scale) and +96 (mode); `LOCKSTEP_ANIM_BONES` infers them. Add
  them to the next recording.
- **The bones' rest path** (state flag bit 2, 80158C84..80158D98): written from the code, never
  met on the home replay; check on the club replay.
- **The scalar channels** (node words 5, 6; 801BD7BC): never read on the home replay; not built.
- **One Sim animator speed** of 17,198 (an Animate the run list misses).
