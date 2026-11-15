# Open problems

- **The bones' controller scale and mode word** (stage 4.4, iteration 186): the '<' record does
  not log the controller's +84 (scale) and +96 (mode); `LOCKSTEP_ANIM_BONES` infers them. Add
  them to the next recording.
- **The bones' rest path** (state flag bit 2, 80158C84..80158D98): written from the code, never
  met on the home replay; check on the club replay.
- **The scalar channels** (node words 5, 6; 801BD7BC): never read on the home replay; not built.
- **One Sim animator speed** of 17,198 (an Animate the run list misses).
- **The blended (skinned) batches' matrices** (stage 4.4e): 642 batches with 3 or 4 slots draw
  with no world matrix; the 'p' cap (20,000) is spent before any slot load. Raise the cap or
  start the 'p' records at the tick, and log the slot loads.
- **Draws past the strip cap** ('w', 8,000): only ticks 1000-1005 tie to models; 1,934 gaps hold
  several world matrices, and display-list ('g') draws are not tied to parts.
