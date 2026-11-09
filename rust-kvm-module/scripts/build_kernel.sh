#!/bin/bash
# One-time: Rust-enabled kernel for ktvmx. Review before running; it creates
# a git worktree in your linux_mainline repo and runs a long build.
set -euo pipefail
SRC=${SRC:-$HOME/WS/linux_mainline}
DST=${DST:-$HOME/WS/linux_ktvmx}
[ -d "$DST" ] || git -C "$SRC" worktree add --detach "$DST" HEAD
cp "$SRC/.config" "$DST/.config"
cd "$DST"
make LLVM=1 rustavailable
scripts/config -e RUST -d KVM_INTEL -d KVM_AMD -d KVM_HYPERV -d DEBUG_INFO_BTF -d MODVERSIONS \
    -e MODULES -e VIRTIO_FS -e FUSE_FS -e SAMPLES_RUST -d CONFIG_DEBUG_INFO_BTF_MODULES
make LLVM=1 olddefconfig
grep -E "^CONFIG_RUST=y" .config || { echo "CONFIG_RUST did not stick: run make LLVM=1 menuconfig (General setup > Rust support)"; exit 1; }
make LLVM=1 -j"$(nproc)" bzImage rust/ modules_prepare
# External modules link against Module.symvers, which only `make modules` writes.
# ktvmx needs vmlinux exports only, so seed it from vmlinux.symvers (else modpost
# reports every kernel symbol, even call_printk, as undefined).
[ -f Module.symvers ] || cp vmlinux.symvers Module.symvers
echo "kernel ready: $DST/arch/x86/boot/bzImage"
