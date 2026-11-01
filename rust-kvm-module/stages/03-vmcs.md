# Stage 3: The VMCS: fields, encodings, VMCLEAR / VMPTRLD / VMREAD / VMWRITE

Read: SDM Vol. 3C ch. 24 "Virtual Machine Control Structures" (24.2 format of the VMCS
region; 24.11 VMREAD/VMWRITE and software use of the VMCS; "VMCS types: ... launch
state"), Appendix B "Field Encoding in VMCS", Appendix A.9 (IA32_VMX_VMCS_ENUM).
Kernel: `arch/x86/kvm/vmx/vmx_ops.h` (`vmcs_readl`, `vmcs_writel`, `vmcs_clear`,
`vmcs_load`), `arch/x86/kvm/vmx/vmcs.h` (`vmcs_field_width`, `vmcs_field_readonly`),
`arch/x86/include/asm/vmx.h` (`enum vmcs_field`; already in `src/vmx.rs`).
Files: `src/vmcs_fields.rs`, `module/s03_vmcs.rs`. Test: `tests/stage03_vmcs.rs`.
QEMU: `qemu/stage03.sh`. Needs stages 1 and 2.

## Tasks (host half)
1. Decode an encoding: `vmcs_field_access_high`, `_index`, `_type`, `_width`,
   `_readonly`. Look at a handful of constants in `src/vmx.rs` and decode them by
   hand first: `GUEST_RIP` is `0x681e`, `VM_EXIT_REASON` is `0x4402`.
2. `vmcs_field_valid_encoding`, `vmcs_field_bits`, `vmcs_field_high`,
   `vmcs_field_mask`, `vmcs_enum_max_index`.
3. `vmcs_checked_write` / `vmcs_checked_read` on top of `VmcsOps`: refuse malformed and
   unsupported encodings (error 12) and writes to read-only fields (error 13),
   truncate like the hardware. Order matters: *unsupported* beats *read-only*.
4. `vmcs_region_init`: revision id and the shadow-VMCS bit in dword 0, abort indicator
   in dword 1.

## Tasks (kernel half, `module/s03_vmcs.rs`)
5. `vmclear`, `vmptrld`, `vmread`, `vmwrite` as `asm!` (memory operand for the first
   two, registers for the last two; Intel syntax operand order is in the doc comments;
   all of them report through RFLAGS). `HwVmcs` (provided) turns them into a
   `VmcsOps`, so everything from stage 4 on works on real hardware through it.

## Think about
- The VMCS layout inside the 4 KiB region is undefined. Why did Intel not just specify
  offsets? What does that let a CPU do (hint: caching VMCS state on the die until VMCLEAR)?
- Why must you `VMCLEAR` a VMCS before freeing it, before moving it to another CPU, and
  before the *first* VMLAUNCH? What are the three launch states (clear, launched,
  active/current) and which instruction moves between them?
- VMWRITE to `VM_EXIT_REASON` fails; the CPU still lets VMX *hardware* write it on
  every exit. Which IA32_VMX_MISC bit relaxes this, and why does KVM nested need it?
- Why do 64-bit fields have a "high" access type (encoding + 1)? Which processors needed
  it, and what does KVM do instead in `vmcs_writel` on 64-bit hosts?
- `vmcs_checked_*` is a debug aid; the kernel instead checks the field *width* at
  compile time (`vmcs_check16/32/64/l` in `vmx_ops.h`). What does Rust give you to do the
  same (const generics? traits per field type?), and what does that cost you here?
- Natural-width fields are 64 bits on x86-64. Why do they have their own width code at all?
- What would VMREAD of a field with index above the enumerated maximum do on real
  hardware? Why bother enumerating (hint: VMREAD/VMWRITE bitmaps and shadow VMCS, a
  nested virtualization feature)?

## Run in QEMU
```
qemu/stage03.sh
```
Expect `stage03 vmcs: PASS` (the selftest does vmxon, vmclear, vmptrld, writes and reads
`GUEST_RIP` and `GUEST_CS_SELECTOR`, checks that the CPU and your `vmcs_checked_write`
agree about `VM_EXIT_REASON`, then vmclear and vmxoff). `module/s03_vmcs.rs` has not been
compiled by the course author.
