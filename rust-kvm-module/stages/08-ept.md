# Stage 8: EPT, memory virtualization

Read: Intel SDM Vol. 3C, "VMX support for address translation": 28.2 *Extended page table
mechanism*, 28.3.2 *EPT translation* (Tables 28-1 .. 28-6: the entry formats), 28.3.3.1
*EPT misconfigurations and violations*, Table 28-7 *Exit qualification for EPT violations*;
the EPT pointer is Table 25-9 (24.6.11, "Extended-Page-Table Pointer"); `INVEPT` is in Vol. 3C
chapter 30 (VMX instruction reference); capabilities in Appendix A.10 (`IA32_VMX_EPT_VPID_CAP`).
Kernel: `construct_eptp()`, `handle_ept_violation()`, `handle_ept_misconfig()` in
`arch/x86/kvm/vmx/vmx.c`; `__invept()` in `vmx_ops.h`; the page-table building (KVM calls the
entries SPTEs) in `arch/x86/kvm/mmu/spte.c` and `tdp_mmu.c`.
(SDM section numbers are those of the 2023+ editions; the section *titles* are stable, search for them.)
Files: `src/vmx_ept.rs` (host half), `module/s08_ept.rs` (kernel half: `invept`, `ept_enable`,
`ept_handle_violation`). Tests: `tests/stage08_ept.rs`; QEMU: `qemu/stage08.sh`.
Guest: `guest/08_ept.S`.

## Tasks (host half, `cargo test --test stage08_ept`)
1. `ept_entry`, `ept_table_entry`, `construct_eptp`: build leaf entries (R/W/X, memory type,
   2 MiB bit), table pointers and the EPT pointer.
2. `ept_index`, `ept_present`: which 9 bits select the entry at each level; EPT has no P bit.
3. `ept_misconfig_check`: the SDM 28.3.3.1 rules, as a function you can unit-test (the CPU
   would raise `EXIT_REASON_EPT_MISCONFIG` for each of them).
4. `ept_map` (used by `ept_map_page` and `ept_map_2m`): allocate missing tables, refuse
   misaligned or size-mixing mappings, write the leaf last.
5. `ept_translate`: a *software* page walker with the same semantics as the hardware: effective
   permissions are the AND along the path; not-present, permission and misconfig faults.
6. `ept_violation_decode`, `ept_violation_is_unmapped`, `invept_descriptor`.

The memory model: "host physical" addresses are offsets into a `GuestMem` (tests use a `Vec<u8>`),
table pages come from a `BumpAlloc` (provided). The same code runs in the kernel against real
pages via `module/s08_ept.rs::host_phys`.

## Tasks (kernel half, QEMU)
- `invept()`: the instruction (descriptor in memory, type in a register, success from RFLAGS).
- `ept_enable()`: check the capabilities, set the secondary controls (`ENABLE_EPT`,
  `UNRESTRICTED_GUEST`), write `EPT_POINTER`.
- `ept_handle_violation()`: demand paging: read the faulting GPA, map a page behind it, resume.
The selftest runs a *real-mode* guest in 1 MiB of RAM reachable only through your EPT, with one
hole at GPA `0x9000`. It checks that the guest's store faults with the right GPA and qualification,
that mapping the page and resuming completes the store, and that `ept_translate` agrees with what
the hardware did. Real mode needs "unrestricted guest", which in turn needs EPT; that is why this is
the stage where the guest stops being flat protected mode. **Not compiled** by the course author.

## Think about
- Without EPT, KVM needs *shadow page tables*. What does the hypervisor have to intercept
  (CR3 writes, `INVLPG`, #PF) to keep them right, and what does EPT replace that with? What new
  cost does a TLB miss have with EPT (count the memory references of a 4-level guest walk, each
  step itself translated by a 4-level EPT walk)?
- Why does a *violation* (reason 48) tell you the GPA but a *misconfiguration* (reason 49) is a
  different exit reason? Which one is a normal event in a working VMM and which one is always a
  bug in the VMM? (KVM uses misconfigurations on purpose for MMIO: how, and why is that fast?)
- The EPT entry has no "present" bit. What does "present" mean, and why is the all-zero entry the
  only reliable "not present"? What would an entry with W=1, R=0 mean on a CPU without
  execute-only support, and why is it reserved instead of being "write-only"?
- Effective permissions are the AND of all levels (as in x86 paging, but for R/W/X). Where would
  you use a restrictive upper-level entry? (Think of dirty logging for live migration.)
- Bit 8 of the exit qualification distinguishes "the guest's own page-table walk touched this
  GPA" from "the final data access". Which of the two can happen with a guest page table that
  lives in an unmapped GPA, and what must the handler do in that case differently?
- EPT memory type: what does `VMX_EPT_MT_WB` vs `UC` change for guest RAM vs a device BAR, and
  how does it combine with the guest's own PAT/MTRR (the "ignore PAT" bit 6)?
- After you change an entry from *not present* to present, no INVEPT is needed. After you change a
  present entry (revoke write permission, or point it elsewhere) it is. Why the asymmetry? What can
  the CPU have cached, and what does `VMX_EPT_EXTENT_CONTEXT` vs `GLOBAL` flush? What does VPID
  (stage 10) add to this picture?
- Mapping 2 MiB at a time saves tables and TLB entries. What goes wrong if the guest-physical and
  host-physical addresses are not both 2 MiB-aligned (look at your `Misaligned` error and at
  `ept_misconfig_check` for level 2)?
- Our tables hand out host pages from a bump allocator and never free them. What would you need
  (mmu notifiers in the real kernel) if the *host* wanted to swap out or migrate a guest page?

## Run
```
cargo test --test stage08_ept
qemu/stage08.sh        # ktvmx: stage08 ept: PASS
```
When the guest hangs instead of exiting: check that `EPT_POINTER` has memory type 6 and walk
length 3 (`0x...5e`/`0x...1e`), that the tables are *zeroed*, and read `VM_INSTRUCTION_ERROR`
if VMLAUNCH/VMRESUME failed (entry checks of the EPTP and of the "unrestricted guest" rules are in
SDM 26.2.1 / 26.3.1).
