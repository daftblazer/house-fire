"""The behaviour scripts' primitives: what each opcode below 256 is, in each game.

Opcodes are one numbering across the games (checked: wherever a game's handler matches a
named one by its code, it sits at the same opcode). The names are the console engine's own,
`cXObjectImpl::Try<Name>` and `cXPersonImpl::Try<Name>` in The Sims 2's map, given to the
other games' handlers where their code matches (tools: work/analysis, docs/primitives.md).

DISPATCH is each game's switch (`TryElement`): the opcodes an object's and a person's
interpreter accept; a person also accepts an object's (cXPersonImpl falls back to
cXObjectImpl). Read from the executables by tools/dispatch.py, which has the switches'
addresses.

Opcodes 256 and up call another tree: 256-4095 global, 4096-8191 the object's own,
8192-9000 its semi-global file. Node targets 253, 254, 255 are error, return true and
return false (TreeSimImpl::DoNodeAction).
"""

NAMES = {
    0: 'Idle', 1: 'GenericSimCall', 2: 'Expression', 3: 'FindBestAction', 4: 'Grab',
    5: 'Drop', 6: 'ChangeSuit', 7: 'Update', 8: 'Random', 9: 'Burn', 10: 'Tutorial',
    11: 'DistanceTo', 12: 'DirectionTo', 13: 'PushAction', 14: 'FindFunctionalObject',
    15: 'TreeBreak', 16: 'FindGoodLocation', 17: 'IdleForInput', 18: 'KillObject',
    19: 'IsPersonInArea', 20: 'CallFunctionalTree', 21: 'ShowString', 22: 'LookTowards',
    23: 'PlaySound', 24: 'Relationship', 25: 'Budget', 26: 'Relationship2', 27: 'GotoRelative',
    28: 'CallNamedTree', 29: 'SetMotiveDelta', 30: 'GosubFoundAction', 31: 'SetToNext',
    32: 'TestObjectType', 33: 'Find5WorstMotives', 34: 'UIEffect', 35: 'UserEvent',
    36: 'Dialog', 37: 'TestInteractingWith', 39: 'InventoryCommand', 40: 'CameraControl',
    41: 'SetBalloon', 42: 'CreateObject', 43: 'DropOnto', 44: 'Animate', 45: 'GotoRoutingSlot',
    46: 'Snap', 47: 'Reach', 48: 'KillSounds', 49: 'NotifyStackObject', 50: 'MakeActionString',
    51: 'PreloadObject', 52: 'RoomEffect', 53: 'CheckObject', 54: 'GbaCommunication',
    55: 'AnimateObject', 56: 'CompleteGoal', 58: 'HUDAddItem', 59: 'HUDManage', 60: 'HUDData',
    61: 'ParticleEffect', 62: 'SocialMode', 63: 'Memory', 64: 'IdleAnimate',
    65: 'PassiveInfluence', 66: 'WantsAndFearsNotify', 67: 'ChangeWantFearTree',
}

# Where a game's handler for an opcode is not the shared one, or the shared one is unknown.
GAME_NAMES = {
    'GUBE69': {57: 'HUDCreate'},
    # Castaway names its primitives in its ELF's symbols (cXObject/cXPerson::Try*). Where its
    # name differs from the one above, or names one we had not, it is its own:
    'R42E69': {21: 'SyncAudio', 39: 'ListenForInput', 47: 'NotifyPlans', 54: 'ManageClothing',
               57: 'HUDCreate', 58: 'HUDAddItem', 59: 'HUDManage', 60: 'HUDData', 68: 'CustomUI',
               69: 'LotTransition', 70: 'FamilyInventory', 71: 'FollowSims', 72: 'SnapObject'},
}

DISPATCH = {
    'G4ME69': {'object': [0, 1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 28, 31, 32, 33, 34, 35, 36, 41, 42, 43, 46, 48, 49, 50, 51, 52, 53, 54, 55, 56],
               'person': [3, 6, 17, 22, 27, 29, 30, 37, 44, 45, 46, 47, 57]},
    'GUBE69': {'object': [0, 1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 28, 31, 32, 33, 34, 35, 36, 39, 40, 41, 42, 43, 46, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61],
               'person': [3, 6, 17, 22, 27, 28, 29, 30, 37, 38, 44, 45, 46, 47, 100]},
    'G4ZE69': {'object': [0, 1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34, 35, 36, 39, 40, 41, 42, 43, 44, 46, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 63, 65, 66, 67],
               'person': [3, 6, 17, 22, 27, 29, 30, 37, 38, 44, 45, 46, 47, 62, 64, 100]},
    'R42E69': {'object': [0, 1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 35, 36, 39, 40, 41, 42, 43, 44, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 63, 65, 66, 67, 68, 69, 70, 72],
               'person': [3, 6, 17, 22, 27, 29, 30, 37, 38, 44, 45, 62, 64, 71]},
    'G4OE69': {'object': [0, 1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 23, 24, 25, 26, 28, 29, 31, 32, 33, 35, 36, 39, 40, 41, 42, 43, 44, 46, 48, 49, 50, 51, 52, 53, 55, 56, 57, 58, 59, 60, 61, 63, 65, 66, 67, 68, 69, 70],
               'person': [3, 6, 17, 22, 27, 28, 29, 30, 37, 38, 44, 45, 62, 64, 71]},
}


def name(game, op):
    """A node's opcode as a name: a primitive, or the tree it calls."""
    if op >= 8192:
        return f'semiglobal:{op:#06x}'
    if op >= 4096:
        return f'private:{op:#06x}'
    if op >= 256:
        return f'global:{op:#06x}'
    return GAME_NAMES.get(game, {}).get(op) or NAMES.get(op) or f'prim{op}'


def accepted(game):
    d = DISPATCH[game]
    return set(d['object']) | set(d['person'])
