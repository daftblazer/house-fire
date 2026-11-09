# Behaviour script primitives

The opcode a BHAV node's primitive number selects, from the dispatch in each executable
(`TryElement`, a switch on the opcode; `cXObjectImpl::TryElement` and `cXPersonImpl::TryElement`
in The Sims 2, `fn_8021242C` and `fn_802246B0` in Bustin' Out). The Sims 2's names are from its map;
Bustin' Out's handler is the function its case calls, named where its code matches The Sims 2's
("inline": the case does the work itself; "-": no case).

Bustin' Out also has a switch on opcodes 100-142 (`fn_801E17BC`), not yet looked at.


## Object primitives

| Opcode | The Sims 2 | Bustin' Out | Matched as |
|---|---|---|---|
| 0 | TryIdle | 80210024 | TryIdle |
| 1 | TryGenericSimCall | 8020D274 |  |
| 2 | TryExpression | 80213890 |  |
| 4 | TryGrab | 802103A8 |  |
| 5 | TryDrop | 80215E3C | TryDrop |
| 7 | TryUpdate | 8021017C | TryUpdate |
| 8 | TryRandom | 8021062C |  |
| 9 | TryBurn | 8020A070 | TryBurn |
| 10 | TryTutorial | 80209C88 | TryTutorial |
| 11 | TryDistanceTo | 802119CC | TryDistanceTo |
| 12 | - | 80211B5C |  |
| 13 | TryPushAction | 80208338 | TryPushAction |
| 14 | - | 8020CC8C |  |
| 15 | TryTreeBreak | 802105C8 | TryTreeBreak |
| 16 | TryFindGoodLocation | 802053A4 | TryFindGoodLocation |
| 18 | TryKillObject | 8020FDF0 | TryKillObject |
| 19 | TryIsPersonInArea | 80204BF8 |  |
| 20 | TryCallFunctionalTree | 8020D098 | TryCallFunctionalTree |
| 21 | TryShowString | 8020FCD0 | TryShowString |
| 23 | TryPlaySound | 802135D0 |  |
| 24 | TryRelationship | 802095B8 | TryRelationship |
| 25 | TryBudget | 8020BEA4 | TryBudget |
| 26 | TryRelationship2 | 802089D8 | TryRelationship2 |
| 28 | TryCallNamedTree | 80206244 |  |
| 29 | TrySetMotiveDelta | - |  |
| 31 | TrySetToNext | 8020C024 | TrySetToNext |
| 32 | - | 80204AE0 | TryTestObjectType |
| 33 | TryFind5WorstMotives | 802087C8 | TryFind5WorstMotives |
| 34 | TryUIEffect | 80204AD4 |  |
| 35 | TryUserEvent | 80204660 |  |
| 36 | TryDialog | inline |  |
| 39 | TryInventoryCommand | - |  |
| 40 | TryCameraControl | - |  |
| 41 | - | inline |  |
| 42 | TryCreateObject | inline |  |
| 43 | TryDropOnto | 8020BC50 | TryDropOnto |
| 44 | TryAnimate | - |  |
| 46 | TrySnap | 80216080 | TrySnap |
| 48 | TryKillSounds | 802134A8 |  |
| 49 | TryNotifyStackObject | 80211F34 |  |
| 50 | - | 80208140 |  |
| 51 | TryPreloadObject | 8020BB24 | TryPreloadObject |
| 52 | TryRoomEffect | 80210780 | TryRoomEffect |
| 53 | TryCheckObject | 80210A4C |  |
| 54 | TryGbaCommunication | 80210B60 |  |
| 55 | - | 80210C94 | TryAnimateObject |
| 56 | TryCompleteGoal | 80211338 |  |
| 58 | TryHUDAddItem | - |  |
| 59 | TryHUDManage | - |  |
| 60 | TryHUDData | - |  |
| 61 | TryParticleEffect | - |  |
| 63 | TryMemory | - |  |
| 65 | TryPassiveInfluence | - |  |
| 66 | TryWantsAndFearsNotify | - |  |
| 67 | TryChangeWantFearTree | - |  |

## Person primitives

| Opcode | The Sims 2 | Bustin' Out | Matched as |
|---|---|---|---|
| 3 | TryFindBestAction | 802266BC |  |
| 6 | TryChangeSuit | 80225EDC |  |
| 17 | TryIdleForInput | 80226378 |  |
| 22 | TryLookTowards | 8021F15C | TryLookTowards |
| 27 | TryGotoRelative | 80223BA0 | TryGotoRelative |
| 29 | TrySetMotiveDelta | 8021EC58 | TrySetMotiveDelta |
| 30 | TryGosubFoundAction | 8021EFB0 | TryGosubFoundAction |
| 37 | TryTestInteractingWith | 80225E50 | TryTestInteractingWith |
| 38 | ? | - |  |
| 44 | ? | inline |  |
| 45 | TryGotoRoutingSlot | 8021FE94 | TryGotoRoutingSlot |
| 47 | TryReach | 80224204 |  |
| 57 | - | inline |  |
| 62 | TrySocialMode | - |  |
| 64 | TryIdleAnimate | - |  |
| 100 | ? | - |  |
