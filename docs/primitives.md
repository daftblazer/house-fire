# Behaviour script primitives

A BHAV node's opcode below 256 is a primitive; 256 and up call another tree (256-4095
global, 4096-8191 the object's own, 8192 and up its semi-global file). The tables are in
`tools/primitives.py`: the names (the console engine's own, `cXObjectImpl::Try<Name>` and
`cXPersonImpl::Try<Name>` in The Sims 2's map) and, per game, the opcodes its interpreter
accepts, read from its executable by `tools/dispatch.py`.

## One numbering

The four games with scripts (The Sims keeps its objects another way, `survey.md`) number
the primitives alike: wherever a game's handler matches a named one by its code, it is at
the same opcode. Each game adds its own at the top:

| | Object switch | Person switch | Accepted | Highest |
|---|---|---|---|---|
| Bustin' Out | `8021242C` | `802246B0` | 55 | 57 (person) |
| The Urbz | `80165964` | `80180E1C` | 63 | 61, and person 100 |
| The Sims 2 | `80109770` | `80124ADC` | 69 | 67, and person 100 |
| Pets | `800EC050` | `80105D48` | 68 | 70, and person 71 |

Opcode 57 is the one known exception to one-name-per-opcode: a person primitive in Bustin'
Out, `HUDCreate` in The Urbz. The Sims 2 adds goals (56), the HUD (58-60), particles (61),
social mode (62), memories (63), idle animation (64), passive influence (65), wants and
fears (66, 67); Pets 68-71, not named yet.

## What the scripts use (`tools/bhav_text.py`)

Every script on the four discs checked: node targets are nodes of their tree or a return
(253 false, 254 true, 255 error), every primitive used is one its game accepts (one dead
script in Pets names the Game Boy link, 54, which Pets dropped), and every call to an
object's own tree finds it (four from semi-global trees, which resolve in the calling
object).

| | Trees | Primitive nodes | Expression | Animate | Calls (global / own / semi-global) |
|---|---|---|---|---|---|
| Bustin' Out | 9,529 | 96,505 | 72% | 5% | 6,060 / 7,797 / 4,554 |
| The Urbz | 4,907 | 51,227 | 68% | 9% | 5,602 / 2,129 / 4,108 |
| The Sims 2 | 5,148 | 59,289 | 69% | 10% | 6,440 / 1,551 / 4,349 |
| Pets | 5,272 | 61,606 | 70% | 10% | 7,321 / 1,898 / 3,878 |

So the expression primitive (2) is most of what the interpreter does; then animation (44),
change suit (6), sounds (23), set-to-next (31), dialogs (36), random (8). A few opcodes are
accepted and never used by any script (Bustin' Out: 53, 57).
