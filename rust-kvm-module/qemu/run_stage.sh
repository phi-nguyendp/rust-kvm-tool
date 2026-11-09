#!/bin/bash
# usage: qemu/run_stage.sh N   -- build ktvmx.ko with STAGE=N, boot nested-VMX QEMU,
# copy the module in over ssh, insmod selftest=N, check dmesg for PASS lines of
# stages 1..N. Machine settings (KERNEL, DISK, ...): see qemu/common.sh.
set -uo pipefail
N=${1:?stage number}
. "$(dirname "$0")/common.sh"

check_inputs
build_module "$N"
boot_vm
to_guest "$HERE/module/ktvmx.ko"
OUT=$("${SSH[@]}" "dmesg -C; insmod /tmp/ktvmx.ko selftest=$N; rmmod ktvmx; dmesg")
echo "$OUT" | grep -E "ktvmx|BUG|Oops|WARNING|panic|RIP:"

fail=0
for i in $(seq "$N"); do
    echo "$OUT" | grep -q "ktvmx: stage$(printf %02d "$i") .*: PASS" ||
        { echo "stage $i: not PASS"; fail=1; }
done
[ $fail = 0 ] && echo "stages 1..$N: PASS"
exit $fail
