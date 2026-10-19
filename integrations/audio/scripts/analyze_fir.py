#!/usr/bin/env python3
"""Analyze Nahimic DeviceOptimization FIR: magnitude response at target freqs.
Pure-python DFT (no numpy). Usage: python3 -I analyze_fir.py <nsx> <outprefix>"""
import base64
import json
import math
import struct
import sys
import wave
import xml.etree.ElementTree as ET


def get_filter(tree, name):
    for el in tree.iter():
        if el.tag.split("}")[-1] == name:
            v = el.find("Value")
            if v is not None and v.text:
                return base64.b64decode(v.text.strip())
    return None


def dft_mag(taps, fs, f):
    w = 2.0 * math.pi * f / fs
    re = im = 0.0
    for n, h in enumerate(taps):
        ang = w * n
        re += h * math.cos(ang)
        im -= h * math.sin(ang)
    return math.hypot(re, im) / len(taps) * 2.0  # normalized like FFT scale


def main():
    path, outprefix = sys.argv[1], sys.argv[2]
    fs = 48000
    tree = ET.parse(path)
    data = get_filter(tree, "kSet_DeviceOptimizationFilterFL")
    taps = list(struct.unpack("<%df" % (len(data) // 4), data))
    print("taps: %d  dc-sum: %.5f" % (len(taps), sum(taps)))

    target = [20, 30, 40, 50, 60, 80, 100, 125, 150, 200, 250, 300, 400, 500,
              630, 800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300,
              8000, 10000, 12500, 16000, 20000]
    rows = []
    for f in target:
        m = dft_mag(taps, fs, f)
        if m <= 0:
            m = 1e-12
        rows.append((f, 20 * math.log10(m)))
    ref = dict(rows)[1000]
    print("gain @1k: %.2f dB" % ref)
    print("%7s %9s %9s" % ("Hz", "abs dB", "rel dB"))
    for f, a in rows:
        print("%7.0f %9.2f %9.2f" % (f, a, a - ref))

    with open(outprefix + ".json", "w") as fh:
        json.dump({"sample_rate": fs, "n_taps": len(taps),
                   "gain_db_at_1k": round(ref, 3),
                   "response": [{"freq": f, "abs_db": round(a, 2), "rel_db": round(a - ref, 2)} for f, a in rows]}, fh, indent=1)
    with open(outprefix + ".f32", "wb") as fh:
        fh.write(data)
    with wave.open(outprefix + ".wav", "wb") as fh:
        fh.setnchannels(1)
        fh.setsampwidth(4)
        fh.setframerate(fs)
        fh.writeframes(data)
    print("wrote", outprefix + ".{json,f32,wav}")


if __name__ == "__main__":
    main()
