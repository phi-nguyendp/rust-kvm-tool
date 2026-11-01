#!/bin/bash
# Stage 10 in QEMU: stages 1..10 selftests (the bitmap test runs guest/10_bitmaps.S:
# `out 0x80` and `rdmsr 0x174` must NOT exit; `out 0x3f8` and `rdmsr EFER` must).
exec "$(dirname "$0")/run_stage.sh" 10
