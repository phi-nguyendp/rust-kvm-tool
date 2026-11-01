#!/bin/bash
# Stage 11 userspace round trip: boots the nested-VMX guest, loads ktvmx.ko, and runs
# examples/ktvmx_run against /dev/ktvmx with guest/07_hello.bin; expects the guest's
# string on stdout. Same boot recipe as qemu/run_stage.sh (see docs/setup.md).
set -uo pipefail
WS=${WS:-$HOME/WS}
KERNEL=${KERNEL:-$WS/linux_ktvmx}
IMAGE=${IMAGE:-$WS/stretch-image}
HERE=$(cd "$(dirname "$0")/.." && pwd)
REL=${HERE#$HOME/}
SSH="ssh -i $IMAGE/bookworm.id_rsa -p 10021 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@127.0.0.1"

make -C "$HERE/module" KDIR="$KERNEL" >/dev/null || { echo "module build failed"; exit 2; }
make -C "$HERE/guest" >/dev/null || { echo "guest build failed"; exit 2; }
# Static, so it does not care about the guest image's glibc.
(cd "$HERE" && RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --example ktvmx_run \
    --target x86_64-unknown-linux-gnu) >/dev/null || { echo "example build failed"; exit 2; }

(cd "$WS" && bash start_virtiofd.sh) &
VFS=$!
sleep 1
qemu-system-x86_64 -m 2G -cpu host,vmx=on -smp 1 -enable-kvm -nographic \
    -object memory-backend-file,id=mem,size=2G,mem-path=/dev/shm,share=on -numa node,memdev=mem \
    -kernel "$KERNEL/arch/x86/boot/bzImage" \
    -net user,host=10.0.2.10,hostfwd=tcp:127.0.0.1:10021-:22 -net nic,model=virtio-net-pci \
    -drive file="$IMAGE/bookworm.img",format=raw,if=virtio \
    -chardev socket,id=char0,path="$WS/vhostqemu" \
    -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=myfs \
    -pidfile "$HERE/qemu.pid" \
    -append "root=/dev/vda console=ttyS0 net.ifnames=0 panic=-1 oops=panic" >"$HERE/qemu.log" 2>&1 &
for i in $(seq 60); do $SSH true 2>/dev/null && break; sleep 2; done
OUT=$($SSH "mkdir -p /mnt && mount -t virtiofs myfs /mnt 2>/dev/null; \
    insmod /mnt/$REL/module/ktvmx.ko; ls -l /dev/ktvmx; \
    /mnt/$REL/target/x86_64-unknown-linux-gnu/release/examples/ktvmx_run /mnt/$REL/guest/07_hello.bin 2>&1; \
    dmesg | grep ktvmx | tail -20; rmmod ktvmx")
echo "$OUT"
kill "$(cat "$HERE/qemu.pid")" 2>/dev/null; kill $VFS 2>/dev/null
echo "$OUT" | grep -q "Hello from the guest" && { echo "stage 11 userspace: PASS"; exit 0; }
echo "stage 11 userspace: FAIL"; exit 1
