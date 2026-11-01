#!/bin/bash
# Stage 11 in QEMU, kernel selftest only: the internal ktvmx_* path runs guest/07_hello.S.
# For the real userspace round trip (/dev/ktvmx + ioctls + mmap) use stage11_userspace.sh.
exec "$(dirname "$0")/run_stage.sh" 11
