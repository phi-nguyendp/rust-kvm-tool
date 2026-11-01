#!/bin/bash
# usage: qemu/run_stage.sh N   -- boot nested-VMX QEMU, load ktvmx.ko selftest=N,
# check dmesg for PASS lines of stages 1..N. Condensed from ~/WS/start_qemu.sh.
set -uo pipefail
N=${1:?stage number}
WS=${WS:-$HOME/WS}
KERNEL=${KERNEL:-$WS/linux_ktvmx}
IMAGE=${IMAGE:-$WS/stretch-image}
HERE=$(cd "$(dirname "$0")/.." && pwd)
SSH="ssh -i $IMAGE/bookworm.id_rsa -p 10021 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@127.0.0.1"

make -C "$HERE/module" KDIR="$KERNEL" >/dev/null || { echo "module build failed"; exit 2; }
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
OUT=$($SSH "mkdir -p /mnt && mount -t virtiofs myfs /mnt 2>/dev/null; insmod /mnt/${HERE#$HOME/}/module/ktvmx.ko selftest=$N; dmesg | grep ktvmx; rmmod ktvmx")
echo "$OUT"
kill "$(cat "$HERE/qemu.pid")" 2>/dev/null; kill $VFS 2>/dev/null
fail=0
for i in $(seq "$N"); do printf "ktvmx: stage%02d" "$i" | xargs -I{} sh -c "echo '$OUT' | grep -q '{}.*PASS'" || { echo "stage $i: not PASS"; fail=1; }; done
exit $fail
