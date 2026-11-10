"""Reading the games' executables (Gekko PowerPC, DOL) for what the engine needs to know:
where a switch on a value sends each case, and what each case calls. Nothing here keeps or
prints the games' code; it reports addresses and numbers."""
import struct


def load_dol(d, data=False):
    """{address: word} for the text sections of a DOL (and with data=True its data
    sections too, for jump tables)."""
    offs = struct.unpack('>18I', d[0:72]); addrs = struct.unpack('>18I', d[72:144]); sizes = struct.unpack('>18I', d[144:216])
    m = {}
    for i, (o, a, s) in enumerate(zip(offs, addrs, sizes)):
        if i < 7 or data:
            for j in range(0, s - 3, 4):
                m[a + j] = struct.unpack('>I', d[o + j:o + j + 4])[0]
    return m


def jump_table_cases(M, start, end):
    """A switch compiled as a jump table: `cmplwi rX, N` (with a branch past it to the
    default; a value first lessened by k with `addi` is taken back up by k),
    default), then `lis`/`addi` of the table and `lwzx`/`mtctr`/`bctr`. Returns {case:
    target} for the cases whose target is not the default, or {} if there is no table."""
    n = table = default = None
    hi = None
    bias = 0                                             # `addi rX, rY, -k` before the compare
    for a in range(start, end, 4):
        w = M.get(a, 0)
        op = w >> 26
        if op == 14 and n is None and sext(w & 0xFFFF, 16) < 0 and (w >> 16 & 31) != 1:
            bias = -sext(w & 0xFFFF, 16)                 # the case is the value minus k
        elif op == 10 and n is None:                     # cmplwi crf, rA, UIMM
            n = w & 0xFFFF
        elif op == 16 and n is not None and default is None:  # bc: the branch to the default
            default = a + sext(w & 0xFFFC, 16)
        elif op == 15 and n is not None:                 # lis rD, hi
            hi = (w & 0xFFFF) << 16
        elif op == 14 and hi is not None:                # addi rD, rA, lo
            table = (hi + sext(w & 0xFFFF, 16)) & 0xFFFFFFFF
        elif w == 0x4E800420 and table is not None:      # bctr
            break
    if n is None or table is None:
        return {}
    out = {}
    for k in range(n + 1):
        t = M.get(table + 4 * k)
        if t is not None and t != default:
            out[k + bias] = t
    return out


def sext(v, b):
    return v - (1 << b) if v & (1 << (b - 1)) else v


def switch_cases(M, start, end, span=4096):
    """A binary-search switch (cmpwi/cmplwi rX,N then beq/bgt/blt, nested) from the first
    compare after `start`: {case value: address of its code}. The value's possible range
    and the values already ruled out are carried down each branch; code reached when one
    value is left is that value's case."""
    cases = {}
    seen = set()
    reg = [None]

    def remaining(lo, hi, excl):
        if hi - lo > span:
            return None
        r = [v for v in range(lo, hi + 1) if v not in excl]
        return r[0] if len(r) == 1 else None

    def walk(pc, lo, hi, excl, last):
        while start <= pc < end:
            key = (pc, lo, hi, excl, last)
            if key in seen:
                return
            seen.add(key)
            w = M.get(pc, 0); op = w >> 26
            if op in (10, 11) and ((w >> 23) & 7) == 0:     # cmp(l)wi cr0, rX, N
                r = (w >> 16) & 31
                if reg[0] is None:
                    reg[0] = r
                if r != reg[0]:
                    break
                last = sext(w & 0xFFFF, 16) if op == 11 else w & 0xFFFF
                pc += 4; continue
            if op == 16 and last is not None:                 # bc on cr0
                bo = (w >> 21) & 31; bi = (w >> 16) & 31; t = pc + sext(w & 0xFFFC, 16)
                if bi > 2:
                    break
                taken = (bo & 0x1C) == 0x0C
                if not taken and (bo & 0x1C) != 0x04:
                    break
                N = last
                if bi == 2:   # eq
                    if taken:
                        if lo <= N <= hi and N not in excl:
                            cases.setdefault(N, t)
                        excl = excl | {N}
                    else:     # bne: taken when != N
                        walk(t, lo, hi, excl | {N}, None)
                        cases.setdefault(N, pc + 4); return
                elif bi == 1:  # gt
                    if taken:
                        walk(t, max(lo, N + 1), hi, excl, None); hi = min(hi, N)
                    else:
                        walk(t, lo, min(hi, N), excl, None); lo = max(lo, N + 1)
                else:          # lt
                    if taken:
                        walk(t, lo, min(hi, N - 1), excl, None); lo = max(lo, N)
                    else:
                        walk(t, max(lo, N), hi, excl, None); hi = min(hi, N - 1)
                pc += 4; continue
            if op == 18 and not w & 1:                        # b
                t = pc + sext(w & 0x03FFFFFC, 26)
                v = remaining(lo, hi, excl)
                if v is not None:
                    cases.setdefault(v, t); return
                pc = t; last = None; continue
            if last is not None and op not in (16, 18, 19):   # between a compare and its branch
                pc += 4; continue
            v = remaining(lo, hi, excl)
            if v is not None:
                cases.setdefault(v, pc)
            return

    pc = start
    while pc < end and (M.get(pc, 0) >> 26) not in (10, 11):
        pc += 4
    walk(pc, -(1 << 15), (1 << 15) - 1, frozenset(), None)
    return cases


def first_call(M, t, limit=24):
    """The function the code at `t` calls first, before any other branch or return."""
    for a in range(t, t + 4 * limit, 4):
        w = M.get(a, 0)
        if w >> 26 == 18:
            return a + sext(w & 0x03FFFFFC, 26) if w & 1 else None
        if w == 0x4E800020:
            return None
    return None
