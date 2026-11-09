#!/bin/bash
# Stage 11 userspace round trip: boots the nested-VMX guest, loads ktvmx.ko, and runs
# examples/ktvmx_run against /dev/ktvmx with guest/07_hello.bin; expects the guest's
# string on stdout. Machine settings (KERNEL, DISK, ...): see qemu/common.sh.
set -uo pipefail
. "$(dirname "$0")/common.sh"

check_inputs
build_module 11
make -C "$HERE/guest" >/dev/null || { echo "guest build failed"; exit 2; }
# Static, so it does not care about the guest image's glibc.
(cd "$HERE" && RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --example ktvmx_run \
    --target x86_64-unknown-linux-gnu) >/dev/null || { echo "example build failed"; exit 2; }

boot_vm
to_guest "$HERE/module/ktvmx.ko" \
    "$HERE/target/x86_64-unknown-linux-gnu/release/examples/ktvmx_run" \
    "$HERE/guest/07_hello.bin"
OUT=$("${SSH[@]}" "insmod /tmp/ktvmx.ko; ls -l /dev/ktvmx; \
    /tmp/ktvmx_run /tmp/07_hello.bin 2>&1; \
    dmesg | grep ktvmx | tail -20; rmmod ktvmx")
echo "$OUT"
echo "$OUT" | grep -q "Hello from the guest" && { echo "stage 11 userspace: PASS"; exit 0; }
echo "stage 11 userspace: FAIL"; exit 1
