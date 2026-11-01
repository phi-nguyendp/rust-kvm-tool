# Stage 13 (planned): nested virtualization and APIC virtualization

State: planned, no code yet. A capstone: after this you can read `arch/x86/kvm/vmx/nested.c`
as a peer. Needs stage 12 (a guest that is a Linux kernel) for the end-to-end test: run
`kvm_intel` *inside* your guest and boot a nested guest with it.

Read: SDM Vol. 3C ch. 32 (*Nested Virtualization*, 2023+ editions; older editions fold it
into the VMCS-shadowing and EPT chapters: check the table of contents), 24.10 *VMCS types:
ordinary and shadow*, 25.3 *changes to instruction behaviour in VMX non-root operation*,
28.3 *EPT* (again, for EPT-on-EPT), 29 *APIC virtualization and virtual interrupts*, 30
(*VMX instruction reference*: what VMXON/VMPTRLD/VMREAD/... do when executed by a guest,
and what the errors are). Kernel: `arch/x86/kvm/vmx/nested.c` (`nested_vmx_run`,
`prepare_vmcs02`, `nested_vmx_vmexit`, `nested_vmx_reflect_vmexit`, `nested_vmx_exit_handled`,
`handle_vmread/vmwrite`, `vmcs12.h`, `vmcs_shadow_fields.h`),
`arch/x86/kvm/mmu/paging_tmpl.h` + `mmu.c` for nested EPT, `Documentation/virt/kvm/x86/nested-vmx.rst`.
Papers: *The Turtles Project: Design and Implementation of Nested Virtualization* (OSDI '10).

## Tasks (outline)
1. **Expose VMX to the guest.** CPUID.1:ECX.VMX, `IA32_FEATURE_CONTROL`, the `IA32_VMX_*`
   capability MSRs (a subset of yours: what the *host* CPU can do minus what you cannot
   emulate; stage 1/4's `adjust_vmx_controls` run in reverse), `CR4.VMXE` handling
   (stage 10's `vmx_set_cr4`).
2. **Emulate VMX instructions.** Turn on exits for VMXON/VMXOFF/VMCLEAR/VMPTRLD/VMREAD/
   VMWRITE/VMLAUNCH/VMRESUME/INVEPT/INVVPID (the stage 7 dispatch table already
   knows their `EXIT_REASON_*`). Decode the operand from `VMX_INSTRUCTION_INFO`
   and `EXIT_QUALIFICATION`, read/write guest memory (stage 8's GPA translation), return
   VMfail* in the guest's RFLAGS the way SDM 30.2 prescribes.
3. **vmcs12.** A software structure holding the guest hypervisor's VMCS (its layout is yours;
   `VMREAD/VMWRITE` of any encoding from stage 3's table work on it). VMCS shadowing to
   skip those exits is the optimisation.
4. **vmcs02.** On the emulated VMLAUNCH/VMRESUME, build the VMCS you actually run: guest
   state from vmcs12's guest area, host state = *your* host, controls = merge(vmcs12, yours),
   MSR/I/O bitmaps = OR of both. Run it; an invalid vmcs12 must produce VMfailValid in the
   guest, not a failed real entry. Re-use stage 5's consistency checks on vmcs12.
5. **Exit reflection.** After a VM exit of the nested guest decide: handle it yourself
   (e.g. an EPT violation on your own mapping) or *reflect* it to the guest hypervisor
   (copy exit info to vmcs12, load vmcs12's host state, resume the L1 guest in vmcs01).
6. **EPT on EPT.** The nested guest's physical memory is described by the guest
   hypervisor's EPT: build a *shadow EPT* combining it with yours, and invalidate it on its INVEPT.
7. **APICv** (separate, smaller): enable `virtual-interrupt delivery`, `APIC-register
   virtualization` and posted interrupts so most LAPIC accesses and interrupt injections stop
   exiting (stage 12's emulation is the fallback).

## Think about
- In a nested setup you have *three* levels (L0 you, L1 the guest hypervisor, L2 its guest). Which VMCS
  does the CPU run when L2 executes? Which levels ever see the real `VMXON` region?
- Why can't the L0 hypervisor simply let L1 execute VMLAUNCH natively?
- A VM-exit controls word in vmcs12 asks to exit on `CPUID`; L0 would not exit on CPUID for its own
  reasons. Which side of the merge produces the final control bits for vmcs02, and what about a
  control bit L0 requires but L1 did not ask for? Who gets the exit?
- Why does the architecture define *shadow VMCS* and what does `VMCS link pointer` do there? Which
  VMREAD/VMWRITE still exit with it enabled?
- An L2 `rdmsr` of an MSR that L1 passed through and L0 intercepts: which bitmap wins and why must
  vmcs02's bitmap be the union of the two *intercept* sets?
- How does the architecture keep L1 from noticing it is not on bare metal when it reads
  `IA32_VMX_BASIC`, `IA32_VMX_PROCBASED_CTLS*` and `IA32_VMX_EPT_VPID_CAP`? Which bits do you choose
  to lie about?
