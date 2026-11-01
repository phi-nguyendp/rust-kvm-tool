# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A minimal Virtual Machine Monitor (VMM) written in Rust that boots a raw 64-bit
payload or a Linux `vmlinux` kernel directly on KVM. It's a learning project
(see `BLOG.md` for the accompanying write-up): almost everything is
hand-rolled rather than pulled from an existing VMM crate (`kvm-ioctls`,
`vm-memory`, etc.) — ioctl numbers, register structs, page tables, and the
boot protocol are all implemented explicitly in `src/kvm.rs` so the mechanics
stay visible.

## Commands

- Build: `cargo build`
- Run: `cargo run -- <path-to-elf-or-vmlinux> [initramfs.cpio]` (requires
  `/dev/kvm` access; arg 1 is a 64-bit ELF — a tiny test payload like the one
  in `64bits-baremetal/` or a real `vmlinux`; optional arg 2 is an
  uncompressed newc cpio initramfs)
- Build an initramfs: `python3 tools/mkinitramfs.py --src <busybox/_install> --out 64bits-baremetal/initramfs.cpio`
  (packages a busybox install tree plus device nodes and a small `/init`)
- Test: `cargo test` — the only tests are the bindgen-generated ones inside
  `src/bootparams.rs`; there are no tests for the VMM logic itself.
- Lint: `cargo clippy` (see the "lint" commit in history; there's no
  `clippy.toml`/CI config, so run it manually before committing).

## Architecture

Boot flow, driven from `src/main.rs`: read the target binary from argv →
`VM::new()` → `insert_memory_region()` (2GB, slot 0) → `setup_irqchip()` →
`create_vcpu()` → `enter_long_mode()` → `load_code()` → optional
`load_initrd()` (if argv[2] given) → `run(entry)`.

- **`src/kvm.rs`** — the core of the VMM. Everything KVM-related lives here:
  - Raw KVM ioctl numbers/request codes are defined manually as constants at
    the top of the file (`KVM_CREATE_VM`, `KVM_SET_USER_MEMORY_REGION`, etc.)
    rather than using the `kvm-ioctls` crate. Prefer this style for any new
    ioctl rather than introducing a wrapper crate.
  - `kvm_regs`/`kvm_sregs`/`kvm_segment`/`kvm_run` etc. are hand-defined
    `#[repr(C)]` structs matching the kernel ABI (only `kvm_bindings` FAM
    types like `kvm_cpuid2`/`CpuId` and the `kvm_run` exit-info unions are
    pulled from the `kvm-bindings` crate).
  - `Kvm` (the `/dev/kvm` fd) → `VM` (one VM fd, owns the single memory
    region and the vCPU list) → `VCpu` (one vCPU fd + its mmap'd `kvm_run`
    page). Currently hardcoded to a single memory region and single vCPU.
  - `VCpu::enter_long_mode()` builds identity-mapped page tables directly in
    guest RAM and sets segment/control registers for 64-bit long mode. Fixed
    guest-physical layout to be aware of when touching this code:
    - `0x2000` / `0x3000` / `0x4000`: PML4 / PDPT / PD (2MB huge pages,
      identity mapped)
    - `0x500`: GDT (null/code/data descriptors)
    - `ADDR_BOOT_PARAMS` (`0x10000`): Linux `boot_params` struct
    - `ADDR_CMDLINE` (`0x20000`): kernel command line string
    - `ADDR_TSS` (`0xffff_d000`) / `ADDR_IDENTITY_MAP` (`0xffff_c000`)
  - `VCpu::run()` is the KVM_RUN exit loop: `KVM_EXIT_IO` is dispatched to
    `handle_io()` (emulates just enough of the COM1 UART for
    `console=ttyS0` output), `KVM_EXIT_HLT` ends the run, everything else is
    currently ignored (there's commented-out debug dumping code for
    diagnosing unhandled/failed exits — worth re-enabling instead of
    reinventing when a guest fails to boot).
  - `VM::load_initrd()` copies the cpio to the top of the identity-mapped
    range (`IDENTITY_MAP_LIMIT`, page-aligned) and patches `ramdisk_image` /
    `ramdisk_size` into the already-written `boot_params`, so it must run
    after `load_code()`. No cmdline change is needed: the kernel execs
    `/init` from the initramfs by default. The cmdline is the fixed
    `CMDLINE` const (`console=ttyS0 earlyprintk=ttyS0`).
  - `VM::load_code()` parses the target as an ELF (`xmas_elf`) and copies
    each `PT_LOAD` segment to its physical address in guest RAM, then writes
    the constructed `boot_params` and cmdline into guest memory. This is how
    both the tiny test payloads and a real `vmlinux` are loaded — there's no
    separate flat-binary path.

- **`src/memory.rs`** — `MemoryWrapper`: RAII wrapper around an anonymous
  `mmap`'d region used as guest RAM (backs `userspace_addr` in the KVM memory
  region struct). `munmap`s on drop.

- **`src/bootparams.rs`** — vendored, bindgen-generated bindings for the
  Linux `struct boot_params` (from upstream `linux/arch/x86/include/uapi/asm/bootparam.h`,
  copied via the `linux-loader` crate's generation). Treat as generated code:
  don't hand-edit it beyond what upstream bindgen would produce; its
  `#[test]`s are bindgen's own layout/offset assertions, not project tests.

- **`64bits-baremetal/`** — scratch payloads for exercising the VMM without a
  full kernel: `hello.S`/`hello`/`guest.bin` is a tiny flat-mapped assembly
  program that writes to COM1 directly (see its comments for the fixed
  `0x100000` load-address assumption baked into the code). `vmlinux.bin` (not
  committed) is used to test the real Linux boot path.

## Rules

- The Linux kernel source tree (used to build the `vmlinux` booted by this
  VMM, and the reference for `boot_params`/KVM ABI details) is at
  `/home/phinguyendp/WS/linux_mainline`. Consult it for kernel headers and
  behavior instead of guessing.
- A reference for how a working QEMU guest is launched (kernel, memory,
  devices, cmdline) is `~/WS/start_qemu.sh`. Run from `~/WS`, it boots
  `linux_mainline/arch/x86/boot/bzImage` with `-m 4G -smp 2 -cpu host`, a
  virtio-blk root disk (`stretch-image/bookworm.img`, `root=/dev/vda`),
  virtio-net with user-mode networking, and a vhost-user-fs share; the
  cmdline is `earlyprintk=ttyS0 console=ttyS0 net.ifnames=0 rodata=off
  loglevel=8 ...`. Use it as the baseline when deciding which devices/cmdline
  options this VMM needs to emulate. Note it boots a `bzImage`, whereas this
  VMM loads an uncompressed `vmlinux` ELF.
