#!/usr/bin/env python3
"""Build an uncompressed 'newc' cpio initramfs from a busybox install tree.

This is a standalone asset-building tool — it does not touch any of the
existing Rust VMM code. It just packages an already-built busybox
(`make CONFIG_PREFIX=_install install`) tree, plus a handful of directories,
device nodes and a small /init script, into a cpio archive that the kernel's
initramfs unpacker understands natively (no gzip/compression involved, so
no dependency on the guest kernel's decompressor support).

Usage:
    python3 tools/mkinitramfs.py --src /home/phinguyendp/Rust/busybox/_install \
        --out 64bits-baremetal/initramfs.cpio

The resulting archive is loaded by the VMM via the new, separate
`VM::load_initrd()` path (see src/kvm.rs) and requires no `root=`/`rdinit=`
kernel command-line changes: the kernel's built-in default is to look for
and exec `/init` inside the initramfs, which is exactly what this script
provides.
"""
import argparse
import os
import stat
import sys
import time

MAGIC = b"070701"

# A few standard device nodes so the kernel can open /dev/console for PID 1's
# stdin/stdout/stderr *before* our /init script gets a chance to run (and
# thus before it could mount devtmpfs itself). Numbers match the well-known
# Linux device-number assignments (Documentation/admin-guide/devices.txt).
DEVICE_NODES = [
    # path, mode bits (without S_IFxxx), major, minor
    ("dev/console", 0o600, 5, 1),
    ("dev/null", 0o666, 1, 3),
    ("dev/tty", 0o666, 5, 0),
    ("dev/kmsg", 0o600, 1, 11),
]

EXTRA_DIRS = ["proc", "sys", "dev", "tmp", "mnt", "root"]

# NOTE: this VMM's serial-port emulation (src/kvm.rs, untouched) never
# raises the UART's IRQ, so the *standard interrupt-driven* tty write path
# (what a normal `write()` to /dev/console goes through) stalls forever and
# never appears on the host. Kernel printk output *does* show up, because
# it uses a separate, synchronous/polled console-write path that doesn't
# need an interrupt. So this script logs its progress via /dev/kmsg
# (which feeds back into that same printk path) instead of plain echo, to
# get a working confirmation trail on the serial console despite that
# limitation.
#
# It deliberately does NOT end with `exec /bin/sh`: with no keyboard-input
# emulation either, an interactive shell reads EOF on stdin immediately and
# exits, which kills PID 1 and panics the kernel. Once someone adds IRQ +
# input support to the VMM, swap the final loop below for `exec /bin/sh` to
# get a real interactive console.
INIT_SCRIPT = b"""#!/bin/sh
log() {
    echo "rust-kvm-tool-init: $*" > /dev/kmsg
}

mount -t proc none /proc
mount -t sysfs none /sys
mount -t devtmpfs none /dev 2>/dev/null

log "busybox initramfs loaded"
log "$(/bin/busybox uname -a)"
log "idling here: no IRQ/keyboard-input emulation yet, so an interactive"
log "shell would hit EOF on stdin immediately and panic PID 1 instead"

while true; do
    sleep 3600 2>/dev/null || :
done
"""


def header(name: bytes, mode: int, filesize: int, ino: int, rdevmajor=0, rdevminor=0) -> bytes:
    namesize = len(name) + 1  # cpio filenames are NUL-terminated
    fields = [
        ino,
        mode,
        0,  # uid
        0,  # gid
        1,  # nlink
        int(time.time()),
        filesize,
        0,  # devmajor (of the containing fs, unused by Linux's unpacker)
        0,  # devminor
        rdevmajor,
        rdevminor,
        namesize,
        0,  # check (unused for newc)
    ]
    return MAGIC + b"".join(b"%08X" % f for f in fields)


def pad4(n: int) -> int:
    return (4 - n % 4) % 4


class CpioWriter:
    def __init__(self, out):
        self.out = out
        self.ino = 1

    def _write_entry(self, name: str, mode: int, data: bytes, rdevmajor=0, rdevminor=0):
        name_bytes = name.encode() + b"\0"
        hdr = header(name.encode(), mode, len(data), self.ino, rdevmajor, rdevminor)
        self.ino += 1
        self.out.write(hdr)
        self.out.write(name_bytes)
        self.out.write(b"\0" * pad4(len(hdr) + len(name_bytes)))
        self.out.write(data)
        self.out.write(b"\0" * pad4(len(data)))

    def add_dir(self, name: str, mode=0o755):
        self._write_entry(name.rstrip("/"), stat.S_IFDIR | mode, b"")

    def add_file(self, name: str, data: bytes, mode=0o755):
        self._write_entry(name, stat.S_IFREG | mode, data)

    def add_symlink(self, name: str, target: str):
        self._write_entry(name, stat.S_IFLNK | 0o777, target.encode())

    def add_chardev(self, name: str, mode: int, major: int, minor: int):
        self._write_entry(name, stat.S_IFCHR | mode, b"", rdevmajor=major, rdevminor=minor)

    def close(self):
        self._write_entry("TRAILER!!!", 0, b"")


def add_tree(cpio: CpioWriter, src_root: str):
    """Walk src_root (a busybox `_install` tree) and add every entry,
    preserving directory/file/symlink structure. Never modifies src_root."""
    for dirpath, dirnames, filenames in os.walk(src_root):
        rel_dir = os.path.relpath(dirpath, src_root)
        dirnames.sort()
        filenames.sort()
        for d in dirnames:
            path = os.path.join(dirpath, d)
            rel = os.path.normpath(os.path.join(rel_dir, d)) if rel_dir != "." else d
            if os.path.islink(path):
                cpio.add_symlink(rel, os.readlink(path))
            else:
                cpio.add_dir(rel, stat.S_IMODE(os.lstat(path).st_mode))
        for f in filenames:
            path = os.path.join(dirpath, f)
            rel = os.path.normpath(os.path.join(rel_dir, f)) if rel_dir != "." else f
            if os.path.islink(path):
                cpio.add_symlink(rel, os.readlink(path))
            else:
                with open(path, "rb") as fh:
                    data = fh.read()
                cpio.add_file(rel, data, stat.S_IMODE(os.lstat(path).st_mode))


def build(src: str, out_path: str):
    with open(out_path, "wb") as out:
        cpio = CpioWriter(out)
        for d in EXTRA_DIRS:
            cpio.add_dir(d)
        for path, mode, major, minor in DEVICE_NODES:
            cpio.add_chardev(path, mode, major, minor)
        cpio.add_file("init", INIT_SCRIPT, mode=0o755)
        add_tree(cpio, src)
        cpio.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--src", required=True, help="busybox _install tree to package")
    ap.add_argument("--out", required=True, help="output cpio path")
    args = ap.parse_args()

    if not os.path.isdir(args.src):
        print(f"error: {args.src} is not a directory", file=sys.stderr)
        return 1

    build(args.src, args.out)
    size = os.path.getsize(args.out)
    print(f"wrote {args.out} ({size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
