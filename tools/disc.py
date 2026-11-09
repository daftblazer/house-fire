"""Read files from a GameCube disc image.

.iso is read directly. Other containers (.rvz, .ciso, .gcz) go through decomp-toolkit
(dtk, from DTK= or the PATH): each file asked for is copied once into work/cache/<disc id>/.
"""
import os, struct, subprocess, shutil, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CACHE = ROOT / 'work' / 'cache'

def dtk():
    d = os.environ.get('DTK') or shutil.which('dtk') or shutil.which('dtk-prodg')
    if not d:
        raise SystemExit('dtk not found: set DTK=/path/to/dtk')
    return d

class Disc:
    def __init__(self, path):
        self.path = str(path)
        self.iso = self.path.lower().endswith('.iso')
        boot = self._sys('boot.bin')[:0x440]
        self.id = boot[:6].decode()
        self.title = boot[0x20:0x60].split(b'\0')[0].decode(errors='replace')
        self.files = self._fst()  # path -> (offset or None, size)

    def _cp(self, inner, out):
        if not out.exists():
            out.parent.mkdir(parents=True, exist_ok=True)
            subprocess.run([dtk(), 'vfs', 'cp', f'{self.path}:{inner}', str(out)], check=True,
                           stdout=subprocess.DEVNULL)
        return out

    def _sys(self, name):
        if self.iso:
            with open(self.path, 'rb') as f:
                h = f.read(0x440)
                if name == 'boot.bin': return h
                dol, fst, fsz = struct.unpack('>III', h[0x420:0x42C])
                if name == 'fst.bin':
                    f.seek(fst); return f.read(fsz)
                if name == 'main.dol':
                    f.seek(dol); d = f.read(0x100)
                    offs = struct.unpack('>18I', d[:72]); sizes = struct.unpack('>18I', d[144:216])
                    f.seek(dol); return f.read(max(o + s for o, s in zip(offs, sizes)))
        tmp = CACHE / '_sys' / pathlib.Path(self.path).stem / name
        return self._cp(f'sys/{name}', tmp).read_bytes()

    def _fst(self):
        fst = self._sys('fst.bin')
        n = struct.unpack('>I', fst[8:12])[0]; st = fst[n * 12:]
        out, stack = {}, [(n, '')]
        for i in range(1, n):
            while stack and i >= stack[-1][0]: stack.pop()
            e = fst[i * 12:i * 12 + 12]
            no = int.from_bytes(e[1:4], 'big'); a, b = struct.unpack('>II', e[4:12])
            name = st[no:st.index(b'\0', no)].decode(errors='replace')
            p = (stack[-1][1] if stack else '') + '/' + name
            if e[0]: stack.append((b, p))
            else: out[p.lstrip('/')] = (a, b)
        return out

    def dol(self):
        return self._sys('main.dol')

    def read(self, name):
        off, size = self.files[name]
        if self.iso:
            with open(self.path, 'rb') as f:
                f.seek(off); return f.read(size)
        return self._cp(f'files/{name}', CACHE / self.id / name).read_bytes()
