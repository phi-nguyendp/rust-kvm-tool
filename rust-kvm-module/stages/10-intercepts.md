# Stage 10: choosing what to intercept (I/O + MSR bitmaps, CR access, VPID)

Read: Intel SDM Vol. 3C 25.6.4 *I/O-Bitmap Addresses*, 25.6.9 *MSR-Bitmap Address*,
25.1.3 *Instructions that cause VM exits conditionally*, 25.6.6 *Guest/Host Masks and
Read Shadows for CR0 and CR4*, Table 27-3 *Exit Qualification for Control-Register
Accesses*, 28.1 *Virtual Processor Identifiers (VPIDs)*, 30.3 `INVVPID`; Appendix A.10
(`IA32_VMX_EPT_VPID_CAP`). Kernel: `arch/x86/kvm/vmx/vmx.c` (`vmx_disable_intercept_for_msr`,
`handle_cr`, `vmx_set_cr0`, `vmx_set_cr4`, `allocate_vpid`), `vmx.h`, `vmx_ops.h` (`__invvpid`).
Files: `src/vmx_bitmaps.rs`, `module/s10_bitmaps.rs`. Test: `tests/stage10_bitmaps.rs`.
QEMU: `qemu/stage10.sh`. Guest: `guest/10_bitmaps.S`. (Needs stages 1-9; `VmcsOps` from
stage 3 is how you talk to the VMCS.)

So far (stage 7) the guest exits on *every* `in`/`out`/`rdmsr`/`wrmsr` because the VMCS
asks for it unconditionally. A real VMM wants the opposite default for hot paths: the
guest touches a harmless port or an MSR it owns (`IA32_FS_BASE`, `SYSENTER_*`, ...)
thousands of times per second, and a VM exit costs microseconds. Two bitmaps tell the CPU
which accesses may run without exiting; CR guest/host masks do the same for CR0/CR4 bits;
VPIDs keep the TLB warm across entries.

## Tasks
1. **I/O bitmaps.** `io_bitmap_set` / `io_bitmap_intercepted`: two pages, A for ports
   `0x0000-0x7fff`, B for `0x8000-0xffff`, one bit per port. An access of 2 or 4 bytes
   touches several ports, and one set bit is enough to exit. An access that reaches past
   port `0xffff` always exits.
2. **MSR bitmap.** One page in four 1 KiB quarters (read-low, read-high, write-low,
   write-high). `msr_bitmap_pos`, `msr_bitmap_init_all_intercept`,
   `vmx_disable_intercept_for_msr`, `vmx_enable_intercept_for_msr`, `msr_bitmap_intercepted`.
   MSRs outside `0..=0x1fff` and `0xc0000000..=0xc0001fff` have no bit: they always exit.
3. **CR accesses.** `vmx_decode_cr_access` (Table 27-3), then `vmx_set_cr0` / `vmx_set_cr4`
   (shadow vs hardware value, guest/host mask), `vmx_guest_cr_view`,
   `cr_write_causes_exit`, `lmsw_result`, and `handle_cr_access` tying them together.
4. **VPID.** `vpid_allocator` (16-bit space, 0 reserved), `invvpid_descriptor`,
   `invvpid_type_supported`.
5. **Kernel half** (`module/s10_bitmaps.rs`): `IntercepBitmaps::vmx_setup_bitmaps` points
   the VMCS at your three pages and turns the two controls on. Then wire `Wired` to your
   stage 6/7 vcpu (the trait explains the three things the selftest needs).

The host tests need no root and no KVM. Run `qemu/stage10.sh` for the real thing: the
guest runs `out 0x80; out 0x3f8; rdmsr 0x174; rdmsr EFER; hlt` and the selftest checks
that the *first* exit is the `0x3f8` write and the *next* one is the EFER read.

## Think about
- Why is "all bits set" the only safe initial state of both bitmaps? What does a
  zero-filled MSR bitmap let the guest do on the very first `wrmsr`?
- The CPU only consults the I/O bitmaps if `CPU_BASED_USE_IO_BITMAPS` is set, and ignores
  `CPU_BASED_UNCOND_IO_EXITING` then. What happens to the VM entry if you set
  `USE_MSR_BITMAPS` but leave `MSR_BITMAP` at 0? (Think of stage 5's consistency checks.)
- Why can't a VMM choose to pass through MSR `0x4000_0000` (a KVM paravirtual MSR)?
  Where do such MSRs have to be emulated, and what does the guest get if you inject #GP?
- A guest does `outw %ax, $0x7fff`. Which bitmap pages are consulted? What if the same
  instruction targets `0xffff`?
- Why does the guest/host mask need the bits the VMM *forces* (CR0.NE, CR4.VMXE) and
  not only the bits the VMM wants to watch? What would the guest see in CR4 if
  `CR4_READ_SHADOW` simply equalled `GUEST_CR4`?
- With EPT (stage 8) and unrestricted guest, which CR accesses still need to exit?
  Compare with `vmx_set_cr0` in the kernel and the `KVM_VM_CR0_ALWAYS_ON` constants.
- `INVVPID` types: when would you use single-context vs all-context, and why does the
  kernel check `IA32_VMX_EPT_VPID_CAP` before using each one? What breaks if two vCPUs of
  different VMs share a VPID?
- Why must `VPID 0` never be given to a guest?

## Run
```
cargo test --test stage10_bitmaps       # host, no root
qemu/stage10.sh                         # needs the Rust kernel build, see docs/setup.md
```
If the selftest says "first exit is not an I/O exit", either port `0x80` is still
intercepted (check `io_bitmap_set` for the *clear* case) or step 4 of
`vmx_setup_bitmaps` was skipped and the bitmaps are not in use at all (then `out 0x80`
exits like everything else).
