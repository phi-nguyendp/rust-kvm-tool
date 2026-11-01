# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Purpose

A learning project, not a product: a Codecrafters-style course (same format as
`~/Rust/tcp-learning`) in which the user writes a KVM-like **Intel VMX hypervisor as a
Rust Linux kernel module** (`ktvmx`). Book: Intel SDM Vol. 3C (VMX), Vol. 3A (modes,
paging), Vol. 2 (VM instructions). Kernel reference: `~/WS/linux_mainline`
(`arch/x86/kvm/vmx/*`, `arch/x86/include/asm/vmx.h`, `rust/kernel/*`).
`~/Rust/kvm-learn/rust-kvm-tool` (the user's userspace VMM, uses /dev/kvm) is the
userspace/payload side in stage 11 and an oracle: run a payload under it to see what
real KVM does.

- `src/vmx_*.rs`, `src/vmcs_fields.rs`: skeleton; unfinished parts are `todo!()`
  stubs the user fills in. **Do not implement them unless asked; give hints.**
- `module/sNN_*.rs`: the kernel-only half of each stage (inline asm, devices) plus a
  `selftest()`; same `todo!()` rule.
- `tests/stageNN_*.rs`: host tests per stage; fail (panic in `todo!()`) until done.
- `qemu/stageNN.sh`: in-QEMU test per stage (needs a Rust-enabled kernel build).
- `stages/NN-*.md`: assignment per stage. `COURSE.md`: stage table.
- `guest/*.S`: guest payload sources; the selftests embed the same bytes inline.

## Commands

```
cargo test --test stage01_vmx_caps      # one stage (host, no root, no KVM needed)
cargo test                              # everything
scripts/build_kernel.sh                 # one-time: Rust-enabled kernel, KVM_INTEL off
make -C module                          # build ktvmx.ko
qemu/run_stage.sh N                     # boot QEMU (nested VMX), insmod selftest=N
```
See `docs/setup.md`. Nothing in `module/` or `qemu/` has been compiled or run by the
author of the course materials (no Rust-enabled kernel was available when they were
written): expect to fix small compile errors in `module/hw.rs` once, and say so
instead of assuming it works.

## Architecture: the hardware seam

Stage logic is `no_std` pure Rust over three traits in `src/vmcs_ops.rs`
(`VmcsOps`, `MsrOps`, `GuestMem`). Host tests use `src/mock.rs`; the kernel module
`#[path]`-includes the very same `src/*.rs` files (see `module/ktvmx.rs`) and
implements the traits with real `vmread/vmwrite/rdmsr`. Consequences:
- Stage files may use only `core` (no `std`, no `alloc`, no `HashMap`): fixed arrays.
- Hardware instructions (VMXON, VMLAUNCH, ...) live only in `module/`.
- `src/vmx.rs` mirrors `asm/vmx.h` constants (provided; do not edit).

## Conventions

- Linux kernel naming: `vmx_vcpu_run`, `adjust_vmx_controls`, `setup_vmcs_config`,
  `struct vcpu_vmx`, `vmx_handle_exit`, `EXIT_REASON_*`, `CPU_BASED_*`, ... so each piece maps to
  `arch/x86/kvm/vmx/vmx.c`. Prefer the kernel name over a Rust idiom and say so.
- Teaching is the point: tie each step to SDM sections and the kernel function, and
  explain the "why". Stage docs have Read / Tasks / Think about / Run sections.
- Verify new tests are passable with a reference solution in a scratch copy
  (never commit solutions into this repo).
- Adding a stage: stub + test + `stages/` doc + `COURSE.md` row (+ `module/` part and
  `qemu/` script if it needs hardware).

## Cross-stage interface (kernel halves)

`module/s07..s10` assume `crate::s06_run::vcpu_vmx::{new_flat32(entry_phys, rsp), run() -> u32}`
(a `todo!()` glue task at the end of stage 6) plus `s03_vmcs::HwVmcs`. Stage 11 needs the
root `Ktvmx` in `module/ktvmx.rs` turned into an `InPlaceModule` (a task in its doc).
