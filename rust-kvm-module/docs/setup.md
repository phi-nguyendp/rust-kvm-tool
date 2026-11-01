# Setup: Rust kernel + nested VMX in QEMU

Your normal kernel (`~/WS/linux_mainline`) has `CONFIG_RUST` off and KVM_INTEL
built in, and it is built in-tree (so `O=` builds are refused). `ktvmx` needs its own
kernel build: Rust on, and `KVM_INTEL`/`KVM_AMD` off so nothing else owns VMX
(two owners of VMXON on one CPU is a bad idea).

1. `scripts/build_kernel.sh` creates `~/WS/linux_ktvmx` (a `git worktree` of
   linux_mainline), copies your `.config`, switches Rust on and KVM_INTEL/AMD off,
   and builds `bzImage` + `modules_prepare` + `rust/`. Needs rustc 1.97 + `rust-src`,
   `bindgen`, `clang`/`lld` (all present when written). 2 CPUs: expect a long build.
2. `make -C module` builds `module/ktvmx.ko` against it.
3. `qemu/run_stage.sh N` is `~/WS/start_qemu.sh` condensed: it boots that bzImage with
   `-cpu host,vmx=on -smp 1`, your `stretch-image/bookworm.img` root disk and the
   `virtiofsd` share of `$HOME` (`mount -t virtiofs myfs /mnt` in the guest), ssh on
   127.0.0.1:10021. It then loads the module with `selftest=N` and greps dmesg for
   `ktvmx: stageNN ...: PASS`.

Why `-smp 1`: the module keeps the VMCS "current" on one CPU and does not pin
itself; one vCPU means no migration (think about why later: VMXON region and
VMCS state are per-logical-processor, stage 2/6).
Why nested works at all: this machine is itself a VM with VMX exposed
(`/sys/module/kvm_intel/parameters/nested` = Y), so L1 (QEMU/KVM) gives L2 VMX.

Reading list
- Intel SDM (vol. 3C ch. 23-33 and appendices A, B, C; chapter numbers shift a little
  between SDM editions, the appendices are stable): https://www.intel.com/sdm
- `~/WS/linux_mainline/arch/x86/kvm/vmx/vmx.c`, `vmenter.S`, `vmx_ops.h`, `include/asm/vmx.h`
- `Documentation/virt/kvm/api.rst`, `Documentation/virt/kvm/x86/`
- Hypervisor From Scratch (VMX on Windows, but good prose): https://github.com/sinakarvandi/hypervisor-from-scratch
- Rust for Linux: `samples/rust/rust_misc_device.rs`, `rust/kernel/miscdevice.rs`,
  https://rust-for-linux.com
