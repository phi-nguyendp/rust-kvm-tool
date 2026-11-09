# Sourced by qemu/*.sh: per-machine settings and the boot / ssh / copy helpers.
#
# Settings, in priority order:
#   1. environment variables         (DISK=... qemu/run_stage.sh 2)
#   2. qemu/local.env                (per machine, gitignored; see local.env.example)
#   3. auto-detection below          (first existing candidate wins)
#
#   KERNEL   Rust-enabled kernel build dir with arch/x86/boot/bzImage
#   DISK     raw ext4 root disk image with sshd and root key login
#   SSH_KEY  private key for root@guest       (default: DISK with .img -> .id_rsa)
#   PORT     host port forwarded to guest :22 (default 10021)
#   MEM      guest memory                     (default 2G)
#   KEEP=1   leave the VM running afterwards

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=/dev/null
[ -f "$HERE/qemu/local.env" ] && . "$HERE/qemu/local.env"

WS=${WS:-$HOME/WS}
first_existing() { for f in "$@"; do [ -e "$f" ] && { echo "$f"; return; }; done; echo "$1"; }
KERNEL=${KERNEL:-$(first_existing "$WS/linux_ktvmx")}
DISK=${DISK:-$(first_existing \
    "$WS/stretch-image/bookworm.img" \
    /data/debian-img/trixie.img)}
SSH_KEY=${SSH_KEY:-${DISK%.img}.id_rsa}
PORT=${PORT:-10021}
MEM=${MEM:-2G}

SSH_OPTS=(-i "$SSH_KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
          -o LogLevel=ERROR -o ConnectTimeout=2)
SSH=(ssh "${SSH_OPTS[@]}" -p "$PORT" root@127.0.0.1)

check_inputs() {
    local ok=1 f
    for f in "$KERNEL/arch/x86/boot/bzImage" "$DISK" "$SSH_KEY"; do
        [ -r "$f" ] || { echo "missing: $f"; ok=0; }
    done
    [ $ok = 1 ] || { echo "set KERNEL/DISK/SSH_KEY in qemu/local.env (see local.env.example)"; exit 2; }
}

build_module() {   # build_module STAGE
    make -C "$HERE/module" KDIR="$KERNEL" STAGE="$1" >"$HERE/qemu/build.log" 2>&1 ||
        { grep -E "^error" -A5 "$HERE/qemu/build.log"; echo "module build failed (qemu/build.log)"; exit 2; }
}

# Boot the guest (disk opened with -snapshot: guest writes are thrown away) and wait
# for ssh. -smp 1: the module keeps a VMCS current on one CPU without pinning.
boot_vm() {
    echo "booting $KERNEL on $DISK (log: qemu.log) ..."
    qemu-system-x86_64 -m "$MEM" -cpu host,vmx=on -smp 1 -enable-kvm -nographic \
        -kernel "$KERNEL/arch/x86/boot/bzImage" \
        -drive file="$DISK",format=raw,if=virtio -snapshot \
        -netdev user,id=net0,hostfwd=tcp:127.0.0.1:$PORT-:22 -device virtio-net-pci,netdev=net0 \
        -pidfile "$HERE/qemu.pid" \
        -append "root=/dev/vda console=ttyS0 net.ifnames=0 panic=-1 oops=panic" \
        >"$HERE/qemu.log" 2>&1 &
    QEMU=$!
    trap stop_vm EXIT
    local i
    for i in $(seq 90); do
        "${SSH[@]}" true 2>/dev/null && return
        kill -0 "$QEMU" 2>/dev/null || { echo "qemu exited, see qemu.log"; tail -20 "$HERE/qemu.log"; exit 2; }
        sleep 2
    done
    echo "guest ssh did not come up, see qemu.log (emergency mode? grep FAILED qemu.log)"; exit 2
}

stop_vm() {
    if [ "${KEEP:-0}" = 1 ]; then
        echo "VM kept running: ${SSH[*]}   (kill $QEMU to stop)"
    else
        kill "$QEMU" 2>/dev/null
    fi
}

to_guest() {   # to_guest FILE... : copy into the guest's /tmp
    scp "${SSH_OPTS[@]}" -P "$PORT" -q "$@" root@127.0.0.1:/tmp/ || { echo "scp failed"; exit 2; }
}
