"""Encode 24-bit BMP frames into an optimized, looping GIF (stdlib only).

usage: make_gif.py OUT.gif DELAY_CS LAST_DELAY_CS frame1.bmp frame2.bmp ...

One global 256-color palette (weighted median cut over every frame), no
dithering; each frame after the first stores only the rectangle that changed.
"""
import struct
import sys
from collections import Counter


def read_bmp(path):
    with open(path, "rb") as f:
        data = f.read()
    off, = struct.unpack_from("<I", data, 10)
    w, h = struct.unpack_from("<ii", data, 18)
    bpp, = struct.unpack_from("<H", data, 28)
    assert bpp == 24, f"{path}: {bpp} bpp"
    row = (w * 3 + 3) & ~3
    bottom_up = h > 0
    h = abs(h)
    px = []
    for y in range(h):
        sy = (h - 1 - y) if bottom_up else y
        base = off + sy * row
        r = data[base:base + w * 3]
        px.extend((r[i + 2], r[i + 1], r[i]) for i in range(0, w * 3, 3))
    return w, h, px


def median_cut(counts, n=256):
    boxes = [list(counts.items())]
    while len(boxes) < n:
        # Split the box with the widest weighted channel range.
        best, best_score, best_ch = None, -1, 0
        for i, b in enumerate(boxes):
            if len(b) < 2:
                continue
            for ch in range(3):
                lo = min(c[0][ch] for c in b)
                hi = max(c[0][ch] for c in b)
                weight = sum(c[1] for c in b)
                score = (hi - lo) * weight ** 0.5
                if score > best_score:
                    best, best_score, best_ch = i, score, ch
        if best is None or best_score <= 0:
            break
        b = sorted(boxes.pop(best), key=lambda c: c[0][best_ch])
        total = sum(c[1] for c in b)
        acc, cut = 0, 1
        for k, c in enumerate(b):
            acc += c[1]
            if acc >= total / 2:
                cut = max(1, min(len(b) - 1, k + 1))
                break
        boxes += [b[:cut], b[cut:]]
    pal = []
    for b in boxes:
        wsum = sum(c[1] for c in b)
        pal.append(tuple(round(sum(c[0][ch] * c[1] for c in b) / wsum) for ch in range(3)))
    return pal


def lzw(indices, min_code=8):
    clear, end = 1 << min_code, (1 << min_code) + 1
    out, bits, nbits = bytearray(), 0, 0

    def emit(code, size):
        nonlocal bits, nbits
        bits |= code << nbits
        nbits += size
        while nbits >= 8:
            out.append(bits & 0xFF)
            bits >>= 8
            nbits -= 8

    table = {}
    size, nxt = min_code + 1, end + 1
    emit(clear, size)
    cur = None
    for k in indices:
        if cur is None:
            cur = k
            continue
        key = (cur, k)
        if key in table:
            cur = table[key]
            continue
        emit(cur, size)
        if nxt < 4096:
            table[key] = nxt
            nxt += 1
            if nxt > (1 << size) and size < 12:
                size += 1
        else:
            emit(clear, size)
            table, size, nxt = {}, min_code + 1, end + 1
        cur = k
    if cur is not None:
        emit(cur, size)
    emit(end, size)
    if nbits:
        out.append(bits & 0xFF)
    blocks = bytearray([min_code])
    for i in range(0, len(out), 255):
        chunk = out[i:i + 255]
        blocks += bytes([len(chunk)]) + chunk
    blocks.append(0)
    return bytes(blocks)


def main():
    out, delay, last = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    frames = [read_bmp(p) for p in sys.argv[4:]]
    w, h = frames[0][0], frames[0][1]
    counts = Counter()
    for _, _, px in frames:
        counts.update(px)
    # 255 colors; index 255 = transparent (a pixel the frame leaves as is).
    pal = list(counts) if len(counts) <= 255 else median_cut(counts, 255)
    clear_idx = 255
    lut = {}
    for c in counts:
        lut[c] = min(range(len(pal)), key=lambda i: (pal[i][0] - c[0]) ** 2 * 3 + (pal[i][1] - c[1]) ** 2 * 4 + (pal[i][2] - c[2]) ** 2 * 2)
    pal += [(0, 0, 0)] * (256 - len(pal))
    gif = bytearray(b"GIF89a" + struct.pack("<HHBBB", w, h, 0xF7, 0, 0))
    for c in pal:
        gif += bytes(c)
    gif += b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00"
    prev = None
    for n, (_, _, px) in enumerate(frames):
        idx = [lut[c] for c in px]
        x0, y0, x1, y1 = 0, 0, w, h
        if prev is not None:
            changed = [i for i in range(w * h) if idx[i] != prev[i]]
            if changed:
                ys = [i // w for i in changed]
                xs = [i % w for i in changed]
                x0, y0, x1, y1 = min(xs), min(ys), max(xs) + 1, max(ys) + 1
            else:
                x0, y0, x1, y1 = 0, 0, 1, 1
        if prev is None:
            sub = [idx[y * w + x] for y in range(y0, y1) for x in range(x0, x1)]
        else:
            sub = [idx[y * w + x] if idx[y * w + x] != prev[y * w + x] else clear_idx for y in range(y0, y1) for x in range(x0, x1)]
        d = last if n == len(frames) - 1 else delay
        # Disposal 1 (keep), transparent flag on index 255 after the first.
        flags = 0x04 | (1 if prev is not None else 0)
        gif += b"\x21\xF9\x04" + bytes([flags]) + struct.pack("<H", d) + bytes([clear_idx]) + b"\x00"
        gif += b"\x2C" + struct.pack("<HHHHB", x0, y0, x1 - x0, y1 - y0, 0)
        gif += lzw(sub)
        prev = idx
    gif.append(0x3B)
    with open(out, "wb") as f:
        f.write(gif)
    print(f"{out}: {len(frames)} frames, {len(counts)} colors -> {len([c for c in pal if c])} palette, {len(gif)} bytes")


main()
