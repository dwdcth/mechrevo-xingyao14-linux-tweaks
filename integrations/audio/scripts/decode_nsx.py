#!/usr/bin/env python3
"""Decode base64 payload fields from Nahimic .nsx speaker tuning XML.

Usage: python3 -I decode_nsx.py <nsx-file>
"""
import base64
import struct
import sys
import xml.etree.ElementTree as ET


def try_floats(data):
    n = len(data)
    out = []
    if n % 4 == 0:
        out.append(("float32", struct.unpack("<%df" % (n // 4), data)))
    if n % 8 == 0:
        out.append(("float64", struct.unpack("<%dd" % (n // 8), data)))
    return out


def main(path):
    tree = ET.parse(path)
    root = tree.getroot()
    for el in root.iter():
        tag = el.tag.split("}")[-1]
        if tag == "Settings":
            for s in el:
                name = s.tag.split("}")[-1]
                val_el = s.find("Value")
                if val_el is None:
                    print("%s = %s (inline)" % (name, (s.get("Value") or "").strip()))
                    continue
                raw_txt = (val_el.text or "").strip()
                try:
                    data = base64.b64decode(raw_txt)
                except Exception:
                    data = None
                if data is None:
                    print("%s = %s" % (name, raw_txt[:80]))
                    continue
                print("%s: %d bytes decoded" % (name, len(data)))
                if len(data) <= 64:
                    print("  hex:", data.hex())
                for kind, vals in try_floats(data):
                    if len(vals) <= 40:
                        print("  %s: %s" % (kind, ["%.6g" % v for v in vals]))
                    else:
                        print("  %s: %d values, first 12: %s  last 4: %s"
                              % (kind, len(vals),
                                 ["%.6g" % v for v in vals[:12]],
                                 ["%.6g" % v for v in vals[-4:]]))
        elif tag == "Data":
            for d in el:
                for v in d:
                    print("DATA %s = %s" % (d.tag.split("}")[-1], (v.text or "").strip()))


if __name__ == "__main__":
    main(sys.argv[1])
