#!/bin/sh
# Stop fprintd right before suspend so the fingerprint reader (USB) is
# released and the platform can reach S0ix on this s2idle-only laptop.
# fprintd is D-Bus activated: it restarts on demand after resume.
case "$1" in
    pre) systemctl stop fprintd.service 2>/dev/null || true ;;
esac
