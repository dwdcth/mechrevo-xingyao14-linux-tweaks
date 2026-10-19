#!/usr/bin/env python3
"""Build EasyEffects deliverables from the extracted Nahimic nsx tuning.

Inputs (already on disk):
  nsx:  extracted Nahimic 1D05E004_InternalSpeakers.nsx (read-only use)
Outputs in the target dir:
  mechrevo-star14-nahimic-speaker.wav   peak-normalized 2048-tap FIR, 48 kHz float32
  speaker-correction.json               EasyEffects preset: convolver (faithful copy)
  speaker-correction-eq.json            EasyEffects preset: measured-response parametric EQ approximation
"""
import base64
import json
import math
import struct
import sys
import wave
import xml.etree.ElementTree as ET

NSX = sys.argv[1]
OUTDIR = sys.argv[2]
IR_NAME = "mechrevo-star14-nahimic-speaker.wav"
FS = 48000


def get_b64(tree, name):
    for el in tree.iter():
        if el.tag.split("}")[-1] == name:
            v = el.find("Value")
            if v is not None and v.text:
                return base64.b64decode(v.text.strip())
    raise KeyError(name)


tree = ET.parse(NSX)
data = get_b64(tree, "kSet_DeviceOptimizationFilterFL")
taps = list(struct.unpack("<%df" % (len(data) // 4), data))
assert len(taps) == 2048

# peak normalize
peak = max(abs(t) for t in taps)
taps_n = [t / peak for t in taps]


def write_wav_float32(path, samples, rate):
    fmt = struct.pack("<HHIIHH", 3, 1, rate, rate * 4, 4, 32)  # IEEE float, mono
    frames = struct.pack("<%df" % len(samples), *samples)
    with open(path, "wb") as fh:
        fh.write(b"RIFF")
        fh.write(struct.pack("<I", 4 + 8 + len(fmt) + 8 + len(frames)))
        fh.write(b"WAVEfmt ")
        fh.write(struct.pack("<I", len(fmt)))
        fh.write(fmt)
        fh.write(b"data")
        fh.write(struct.pack("<I", len(frames)))
        fh.write(frames)


wav_path = OUTDIR.rstrip("/") + "/" + IR_NAME
write_wav_float32(wav_path, taps_n, FS)
print("wrote", wav_path)

convolver_preset = {
    "output": {
        "blocklist": [],
        "plugins_order": ["convolver"],
        "convolver": {
            "state": True,
            "bypass": False,
            "input-gain": 0.0,
            "output-gain": 0.0,
            "kernel-name": IR_NAME,
            "ir-width": 100,
            "autogain": True,
        },
    }
}

with open(OUTDIR.rstrip("/") + "/speaker-correction.json", "w") as fh:
    json.dump(convolver_preset, fh, indent=2)
    fh.write("\n")
print("wrote speaker-correction.json")


def band(freq, gain, width, ftype="Bell", slope="x1"):
    return {"gain": gain, "frequency": freq, "width": width, "type": ftype,
            "mode": "RLC (BT)", "slope": slope, "solo": False, "mute": False}


# Values below are readings of the extracted FIR's magnitude response
# (see analysis table); widths are fitted, gains/frequencies are measured.
eq_preset = {
    "output": {
        "blocklist": [],
        "plugins_order": ["filter", "equalizer"],
        "filter": {
            "state": True,
            "bypass": False,
            "input-gain": 0.0,
            "output-gain": 0.0,
            "frequency": 110.0,
            "width": 4.0,
            "quality": 0.0,
            "gain": 0.0,
            "balance": 0.0,
            "type": "High-pass",
            "mode": "RLC (BT)",
            "equal-mode": "IIR",
            "slope": "x2",
        },
        "equalizer": {
            "state": True,
            "bypass": False,
            "mode": "IIR",
            "num-bands": 5,
            "input-gain": 0.0,
            "output-gain": 0.0,
            "split-channels": False,
            "band0": band(250.0, 3.7, 300.0),      # FIR: +3.7 dB @250 Hz (matches nsx BassBoost 250 Hz)
            "band1": band(630.0, -1.2, 500.0),     # FIR: -1.2 dB @630 Hz
            "band2": band(1250.0, 2.0, 800.0),     # FIR: +2.0 dB @1250 Hz
            "band3": band(4000.0, -13.5, 3300.0),  # FIR: -13.5 dB @4 kHz notch region
            "band4": band(11000.0, 5.4, 5000.0),   # FIR: +5.4 dB @10-12.5 kHz
        },
    }
}

with open(OUTDIR.rstrip("/") + "/speaker-correction-eq.json", "w") as fh:
    json.dump(eq_preset, fh, indent=2)
    fh.write("\n")
print("wrote speaker-correction-eq.json")
