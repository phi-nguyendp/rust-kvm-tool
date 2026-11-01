# Stage 11: the userspace ABI, /dev/ktvmx (and driving it with a VMM)

Read: `Documentation/virt/kvm/api.rst` sections 3 (*Extensions*), 4.1-4.17 (system, VM and
vCPU ioctls: `KVM_CREATE_VM`, `KVM_SET_USER_MEMORY_REGION`, `KVM_CREATE_VCPU`, `KVM_RUN`,
`KVM_GET/SET_REGS`, `KVM_GET/SET_SREGS`) and section 5 (*The kvm_run structure*);
`include/uapi/linux/kvm.h`; `include/uapi/asm-generic/ioctl.h`. SDM Vol. 3C 24.4.1 for the
segment access-rights word (Table 24-2). Kernel: `virt/kvm/kvm_main.c` (`kvm_dev_ioctl`,
`kvm_vm_ioctl`, `kvm_vcpu_ioctl`, `kvm_vcpu_mmap`), `arch/x86/kvm/x86.c`
(`kvm_arch_vcpu_ioctl_run`), `vmx.c` (`vmx_set_segment`, `vmx_get_segment`).
Rust for Linux: `samples/rust/rust_misc_device.rs`, `rust/kernel/{miscdevice,uaccess,ioctl}.rs`,
`rust/kernel/mm/virt.rs` (mmap).
Files: `src/vmx_uapi.rs`, `module/s11_uapi.rs`, `examples/ktvmx_run.rs` (provided).
Test: `tests/stage11_uapi.rs`. QEMU: `qemu/stage11.sh`, `qemu/stage11_userspace.sh`.
Guest: `guest/07_hello.S`, built by `guest/Makefile`.

Until now your hypervisor is driven by in-kernel selftests. This stage gives it the API a
VMM needs, and deliberately copies KVM's: same ioctl magic (`0xAE`), same numbers, same
`#[repr(C)]` layouts (`kvm_regs`, `kvm_sregs`, `kvm_segment`) and the same `kvm_run` page.
Then your own `rust-kvm-tool`, which only knows how to speak to `/dev/kvm`, can run guests
on *your* module.

**Design (read this before the code).** KVM has three file descriptors: `/dev/kvm`, a VM
fd and a vCPU fd. We collapse them: *opening `/dev/ktvmx` gives you a VM with one vCPU*,
and all ioctls go to that one fd. `KTVMX_CREATE_VM` / `KTVMX_CREATE_VCPU` exist only so
KVM-shaped code works; they return 0 instead of a new fd (userspace `dup()`s the fd where it
expects one). `KTVMX_SET_USER_MEMORY_REGION` *copies* the region into module-owned RAM
(KVM pins and maps the user's own pages). `mmap` on the fd returns the run page (page 0)
and the PIO data page (page 1).

## Tasks
1. **ioctl numbers.** `ioc`, `ioc_dir`, `ioc_type`, `ioc_nr`, `ioc_size`: rebuild
   `_IOC()` from `asm-generic/ioctl.h` and check it reproduces every KVM number in the
   test (including the sizes of the structs, which are part of the number).
2. **Segments.** `segment_to_ar` / `ar_to_segment`: `kvm_segment`'s separate bytes versus the
   VMCS's packed access-rights word. Mind bit 16 ("unusable").
3. **sregs.** `sregs_to_vmcs` / `vmcs_to_sregs`: eight segments, GDTR/IDTR, CR0/3/4, EFER.
4. **regs.** `regs_to_vmcs` / `vmcs_to_regs`: RIP/RSP/RFLAGS live in the VMCS, the rest in
   your software GPR array (x86 encoding order, same as stage 10's `gprs_t`).
5. **Memory region.** `memslot_check`: the validation `kvm_set_memory_region` does,
   reduced to our one-slot world.
6. **Exits.** `exit_to_run`: write a logical exit into the shared `ktvmx_run` page and,
   for `out`, the data into the PIO page.
7. **Kernel half** (`module/s11_uapi.rs`): write the `ktvmx_*` functions (create, set memory
   into your stage 8 EPT, set/get regs and sregs, the run loop that combines stages 6, 7
   and 9), publish the exit in the run page after `KTVMX_RUN`, implement `mmap`. Make
   `Ktvmx` in `module/ktvmx.rs` an `InPlaceModule` that owns `s11_uapi::register()` (this
   is the one edit outside the stage files; copy the shape of `rust_misc_device.rs`).
8. Run `examples/ktvmx_run.rs` (provided) against it.

## Think about
- The ioctl number encodes the *size* of its argument. What breaks, silently, if you add
  a field to `ktvmx_regs`? Why does the kernel's own `kvm_run` keep a 256-byte union and a
  `padding` member instead of letting the union grow?
- `kvm_segment` has `present`, `unusable` and `type`; the VMCS has one AR word. Which fields
  have no home in the AR word? Which VMCS-only data (e.g. the segment's "accessed" bit)
  must a get/set round trip preserve?
- Why is RFLAGS bit 1 forced on? What happens at VM entry if it is zero? (Stage 5.)
- Why does `KVM_SET_SREGS` write both CR0 and the VMX fixed bits through `vmx_set_cr0` instead
  of writing `GUEST_CR0` directly? What does userspace see from `KVM_GET_SREGS` afterwards?
- After an `in` exit userspace puts the value in the PIO page and calls `KTVMX_RUN` again.
  Where must your run loop copy it, and what goes wrong if the guest executed `insb` (string
  I/O: `count > 1`, RDI-relative) instead of `in`?
- Our `SET_USER_MEMORY_REGION` copies. What can userspace not do afterwards that it can with
  real KVM (think: shared memory with virtio devices)? How would `pin_user_pages_fast` plus
  your stage 8 EPT mapping change the design, and what new problem appears when userspace
  `munmap`s the region?
- Why must `KTVMX_RUN` stay interruptible by signals even though your loop never sleeps?
  What does KVM return and how does a VMM tell "interrupted" from "guest exited"?
- `/dev/ktvmx` is a misc device: who may open it? Compare with `/dev/kvm` permissions.

## Run
```
cargo test --test stage11_uapi          # host: layouts, ioctl numbers, conversions
make -C guest                           # 07_hello.bin, 10_bitmaps.bin, ...
qemu/stage11.sh                         # kernel selftest of the internal path
qemu/stage11_userspace.sh               # /dev/ktvmx + examples/ktvmx_run + hello.bin
```
Expected output of the last one: `Hello from the guest` followed by
`[ktvmx_run] guest halted after 22 exits` (21 characters + the `hlt`).

## Pointing rust-kvm-tool at it
`~/Rust/kvm-learn/rust-kvm-tool/src/kvm.rs` already defines the ioctl numbers, `kvm_regs`,
`kvm_sregs`, `kvm_segment` and a `kvm_run` with the same layout as ours (the first 32 bytes
plus the union; it reads `exit_reason` and `io.{direction,port,data_offset}`). Do not
edit it as part of the course unless you want to; this is the checklist for a
`--backend ktvmx` experiment (one patch, ~20 lines):

1. `Kvm::new()` (line ~626): open `/dev/ktvmx` instead of `/dev/kvm` (make the path an
   argument or an env var).
2. `VM::new()` (~660) and `create_vcpu()` (~746): where `KVM_CREATE_VM` /
   `KVM_CREATE_VCPU` return an fd that is wrapped in an `OwnedFd`, use
   `libc::dup(kvm_fd)` instead of the return value (our ioctls return 0).
3. `KVM_GET_VCPU_MMAP_SIZE` + `mmap` of the vCPU fd work unchanged (`mmap` offset 0).
4. **Ordering:** the tool calls `insert_memory_region()` *before* `load_code()`; our module
   copies the region *at ioctl time*. Either call `KTVMX_SET_USER_MEMORY_REGION` again
   after `load_code()`/`enter_long_mode()` or move it there.
5. `setup_irqchip()` (~678): `KVM_SET_TSS_ADDR`, `KVM_SET_IDENTITY_MAP_ADDR`,
   `KVM_CREATE_IRQCHIP`, `KVM_CREATE_PIT2` and `KVM_SET_CPUID2` return `ENOTTY` from your
   module. Make them non-fatal for flat payloads (or skip them under the new backend).
6. The payload: `64bits-baremetal/hello` is a 64-bit ELF the tool loads at 1 MiB and runs in
   long mode; `enter_long_mode()` builds its page tables in guest RAM and fills `kvm_sregs`
   (the `kvm_segment`s, CR0/3/4, EFER). That exercises your `sregs_to_vmcs` for *long*
   mode, a much stricter test of stage 5's checks than the real-mode `ktvmx_run`.
7. A real Linux `vmlinux` additionally needs an interrupt controller and a timer (stage 12).

## Why not just keep going with rust-kvm-tool for *all* stages?
Because it speaks to `/dev/kvm`, which is the in-kernel hypervisor you are *replacing*:
it can't run VMXON, write a VMCS or take a VM exit by itself. From here it is useful as
the userspace half, and as an oracle: when your module disagrees with real KVM, run the
same payload under both and diff the exit sequences.
