# Stage 5: guest state, host state, and the checks the CPU runs on them

Read: Intel SDM Vol. 3C chapter 24.4 (guest-state area), 24.5 (host-state area), Figure 24-2
(segment access-rights format), 26.2.2-26.2.4 (checks on host state), 26.3.1 (checks on guest
state: CR0/CR4/EFER 26.3.1.1, segment registers 26.3.1.2, descriptor tables, RIP/RFLAGS
26.3.1.4). Vol. 3A 3.4.5 (segment descriptors), 9.8 (switching to long mode), 4.5 (IA-32e paging).
Kernel: `arch/x86/kvm/vmx/vmx.c`: `vmx_segment_access_rights()`, `__vmx_set_segment()`,
`vmx_get_segment()`, `vmx_set_cr0()`, `vmx_set_efer()`, `enter_rmode()`,
`vmx_set_constant_host_state()`, `kvm_vmx_segment_fields[]`.
Files: `src/vmx_state.rs`, `module/s05_state.rs` (provided). Test: `tests/stage05_state.rs`.
Demo: `qemu/stage05.sh`. (Needs stages 1-4.)

VM entry does not "load" a VM: it loads **registers**. The VMCS guest-state area is the CPU
state the guest starts with (CR0/3/4, RIP, RSP, RFLAGS, the full hidden part of every segment
register, GDTR/IDTR/LDTR/TR, EFER...). The host-state area is what the CPU restores on exit.
If any of it is inconsistent the CPU refuses to enter (VM-instruction error 7/8, or an exit with
bit 31 set, stage 6), and the manual's rule list for "inconsistent" is long. This stage builds
guests in the three classic modes and then implements a checker for the rules, so that
`VMLAUNCH` failures can be debugged before the hardware says "no".

## Tasks
1. Segment access rights (the packed AR word): `vmx_segment_access_rights`,
   `vmx_set_segment`, `vmx_get_segment`. Mind the *unusable* bit (16).
2. `vmx_setup_guest_common`: link pointer, activity/interruptibility state, DR7, descriptor
   tables, an unusable LDTR and a usable TR.
3. The three builders: `vmx_setup_guest_real_mode`, `vmx_setup_guest_protected_mode`,
   `vmx_setup_guest_long_mode`. Note what each does to `CR0`, `CR0_READ_SHADOW`,
   `CR0_GUEST_HOST_MASK` and to `VM_ENTRY_CONTROLS`.
4. `vmx_set_host_state`: copy a `host_state` into the host-state area (mind the selector trap).
5. `vmx_check_guest_state`: the first-violated-rule checker. The doc comment lists the rules in
   order and names the `ERR_*` each one returns. Subset of SDM 26.3.1, but real.

The tests build states with your builders, break one field at a time, and expect the matching
`ERR_*`. `module/s05_state.rs` reads the real host state of the CPU it runs on
(`sgdt`, `sidt`, `str`, the selectors, CR0/3/4, the MSRs) and in QEMU runs the checker over a
VMCS written to real hardware.

## Think about
- Why does the guest see `CR0_READ_SHADOW` instead of `GUEST_CR0`? Which bits of `GUEST_CR0`
  and `GUEST_CR4` can the guest not own, and why must `CR4.VMXE` be 1 in `GUEST_CR4`?
- Real mode needs *unrestricted guest*. What did hypervisors do before it existed (virtual-8086
  mode, `enter_rmode()`), and which of the checks in your checker is the reason you cannot
  just set `CR0.PE=0`?
- Why must an *unusable* segment still be written with a clean AR word, and why does
  `vmx_segment_access_rights` map "not present" to "unusable" too?
- SS and CS are checked against each other (RPL, DPL). Which checks are waived with unrestricted
  guest and `CR0.PE=0`, and why is that safe?
- EFER.LMA is *derived* by hardware in a real CPU, yet the VMCS makes you set it and the
  "IA-32e mode guest" entry control. What goes wrong if they disagree?
- A VM exit leaves the host's TR limit, GDTR/IDTR limits, and a few other things in a state
  that is *not* what the host-state area said. Find the list in SDM 27.5 and compare with what
  `vmx_prepare_switch_to_host()` repairs in the kernel.
- Optional extension: write `vmx_check_host_state` (SDM 26.2.2-26.2.4: CR0/CR4 fixed bits, CS and
  TR selectors non-zero, RPL=TI=0, `CR4.PAE` in a 64-bit host, canonical bases, EFER consistent
  with "host address-space size"). What error number does the CPU give for a bad host state?

## Run the demo
```
cargo test --test stage05_state
make -C module && qemu/stage05.sh
```
`dmesg` shows the host state the module read (`ktvmx: host: cs 0x10 ... tr_base ...`). Compare
`tr_base` with the TSS address in `/proc/kallsyms` (`cpu_tss_rw`), and `gdtr_base` with
`cpu_entry_area`.
