# ktvmx: build a KVM-style VMX hypervisor, kernel-style (in Rust)

Codecrafters-style, same format as the `ktcp` TCP course: each stage has an
assignment in `stages/`, stubs marked `todo!()`, and tests that fail until you finish.
Every stage has two halves:

- **Host half** (`src/*.rs`, `tests/stageNN_*.rs`): hardware-independent logic
  (field encodings, capability math, EPT entries, exit decoding...) tested with
  `cargo test` against mock VMCS/MSR/guest-memory. No root, no KVM, no kernel.
- **Kernel half** (`module/sNN_*.rs`, `qemu/stageNN.sh`): the instructions themselves
  (VMXON, VMLAUNCH, ...) in a Rust Linux module, tested by loading it in QEMU with
  nested VMX. Needs the one-time kernel build in `docs/setup.md`.

```
cargo test --test stage01_vmx_caps       # run one stage's host tests
cargo test                               # everything so far
qemu/run_stage.sh 6                      # in-QEMU selftests of stages 1..6
```

Book: Intel SDM Vol. 3C (ch. VMX: VMCS, VM entries/exits, EPT; appendices A/B/C),
Vol. 3A (modes, paging), Vol. 2 (VMXON/VMLAUNCH...). Kernel: `arch/x86/kvm/vmx/`,
`arch/x86/include/asm/vmx.h`. Naming follows the kernel (`vmx_vcpu_run`,
`adjust_vmx_controls`, `EXIT_REASON_HLT`, `CPU_BASED_HLT_EXITING`).

| # | Stage | SDM | Kernel | State |
|---|-------|-----|--------|-------|
| 1 | Capability discovery: CPUID, FEATURE_CONTROL, VMX_BASIC, EPT/VPID cap | Vol 3C 23.6-23.7, App. A | `vmx_disabled_by_bios`, `setup_vmcs_config` | ready |
| 2 | VMXON/VMXOFF: CR fixed bits, VMXON region, VMfail decoding | 23.7, 30.3 | `kvm_cpu_vmxon`, `vmx_hardware_enable` | ready |
| 3 | VMCS: VMCLEAR/VMPTRLD, VMREAD/VMWRITE, field encodings | ch. 24, App. B | `vmcs_load`, `vmcs_clear`, `vmx_ops.h` | ready |
| 4 | Execution controls: allowed-0/1, TRUE_* MSRs, `vmcs_config` | 24.6, 26.2, App. A.3-A.5 | `adjust_vmx_controls`, `setup_vmcs_config` | ready |
| 5 | Guest/host state, segments, modes, consistency checks | 24.4-24.5, 26.3 | `vmx_vcpu_setup`, `vmx_set_segment`, `vmx_set_cr0` | ready |
| 6 | VM entry/exit: VMLAUNCH/VMRESUME asm, exit reasons, first guest (`hlt`) | 26, 27 | `__vmx_vcpu_run`, `vmx_vcpu_run`, `vmx_handle_exit` | ready |
| 7 | Exit handling: CPUID, port I/O (UART), MSRs, skip instruction | 27.2, Tables 27-3/27-5 | `kvm_vmx_exit_handlers`, `handle_io`, `kvm_emulate_cpuid` | ready |
| 8 | EPT: tables, EPTP, violations/misconfig, INVEPT | ch. 28 | `arch/x86/kvm/mmu/`, `handle_ept_violation` | ready |
| 9 | Events: injection, IDT-vectoring, interrupt window, preemption timer | 25.8, 27.2.3, 25.5.1 | `vmx_inject_irq`, `vmx_complete_interrupts` | ready |
| 10 | Intercepts: I/O and MSR bitmaps, CR access, VPID | 25.6, 27 Table 27-3 | `vmx_disable_intercept_for_msr`, `handle_cr` | ready |
| 11 | Userspace ABI (`/dev/ktvmx`, KVM-compatible ioctls) and `ktvmx_run` | - | `kvm_main.c`, `kvm_vcpu_ioctl` | ready |
| 12 | LAPIC/IOAPIC/PIT: enough to boot Linux with rust-kvm-tool | Vol 3A ch. 12 | `arch/x86/kvm/lapic.c` | planned |
| 13 | Nested VMX / APICv | ch. 32/29 | `nested.c` | planned |

Where `rust-kvm-tool` fits: it talks to `/dev/kvm`, i.e. to the in-kernel KVM, so it
cannot exercise your VMX code by itself, and it cannot be used for stages 1-10 (those
are *inside* the kernel). It plays two roles: (1) an **oracle**, run the same payload
under it to see what real KVM reports (exit sequence, register values); (2) from stage 11
on, your module speaks KVM's ioctl ABI with KVM's struct layouts, so the tool can be
pointed at `/dev/ktvmx` by changing the device path (see `stages/11-uapi.md`).
