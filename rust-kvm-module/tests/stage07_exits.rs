use ktvmx::mock::MockVmcs;
use ktvmx::vmcs_ops::VmcsOps;
use ktvmx::vmx::*;
use ktvmx::vmx_exit::*;

const GARBAGE: u64 = 0xdead_beef_0000_0000;

fn host_cpuid(leaf: u32, subleaf: u32) -> [u32; 4] {
    match leaf {
        0 => [0x16, 0x756e_6547, 0x6c65_746e, 0x4965_6e69], // "GenuineIntel"
        1 => [0x000906ea, 0x0010_0800, CPUID_1_ECX_VMX | 0x7ffa_3203, 0xbfeb_fbff],
        7 => [subleaf, 0xaaaa, 0xbbbb, 0xcccc], // echoes the subleaf
        _ => [0; 4],
    }
}

/// A VMCS that has just taken a VM exit for `reason` at RIP 0x1000, on an
/// instruction `len` bytes long.
fn exit(reason: u32, len: u32) -> MockVmcs {
    let mut v = MockVmcs::default();
    v.vmcs_write32(VM_EXIT_REASON, reason);
    v.vmcs_write32(VM_EXIT_INSTRUCTION_LEN, len);
    v.vmcs_writel(GUEST_RIP, 0x1000);
    v
}

fn io_qual(size: u8, is_in: bool, string: bool, rep: bool, imm: bool, port: u16) -> u64 {
    (size as u64 - 1)
        | (is_in as u64) << 3
        | (string as u64) << 4
        | (rep as u64) << 5
        | (imm as u64) << 6
        | (port as u64) << 16
}

fn ctx(uart: &mut uart_com1) -> exit_ctx<'_> {
    exit_ctx { uart, host_cpuid }
}

#[test]
fn skip_advances_rip_and_clears_interrupt_shadow() {
    let mut v = exit(EXIT_REASON_CPUID, 2);
    v.vmcs_write32(
        GUEST_INTERRUPTIBILITY_INFO,
        GUEST_INTR_STATE_STI | GUEST_INTR_STATE_MOV_SS | GUEST_INTR_STATE_NMI,
    );
    skip_emulated_instruction(&mut v);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1002);
    // STI/MOV-SS blocking ends with the instruction; NMI blocking does not.
    assert_eq!(v.vmcs_read32(GUEST_INTERRUPTIBILITY_INFO), GUEST_INTR_STATE_NMI);
}

#[test]
fn decode_io_qualification_fields() {
    // out dx, al  (outb) to COM1
    let x = decode_io_qualification(io_qual(1, false, false, false, false, 0x3f8));
    assert_eq!(
        x,
        io_exit { size: 1, is_in: false, string: false, rep: false, imm: false, port: 0x3f8 }
    );
    // in ax, 0x60  (imm form)
    let x = decode_io_qualification(io_qual(2, true, false, false, true, 0x60));
    assert_eq!(x, io_exit { size: 2, is_in: true, string: false, rep: false, imm: true, port: 0x60 });
    // rep outsd
    let x = decode_io_qualification(io_qual(4, false, true, true, false, 0xcf8));
    assert_eq!(x.size, 4);
    assert!(x.string && x.rep && !x.is_in);
    assert_eq!(x.port, 0xcf8);
    // Bits we do not decode must not leak into the port.
    let x = decode_io_qualification(io_qual(1, false, false, false, false, 0xffff) | 1 << 40);
    assert_eq!(x.port, 0xffff);
}

#[test]
fn uart_collects_thr_writes_and_drops_overflow() {
    let mut u = uart_com1::new();
    u.pio_write(UART_THR, 1, b'o' as u32);
    u.pio_write(UART_THR, 1, 0x1_6b); // only the low byte counts: 'k'
    u.pio_write(0x3f9, 1, b'!' as u32); // IER, not the console
    u.pio_write(0x80, 1, b'!' as u32); // some other device
    assert_eq!(u.output(), b"ok");
    for _ in 0..UART_CONSOLE_SIZE * 2 {
        u.pio_write(UART_THR, 1, b'x' as u32);
    }
    assert_eq!(u.output().len(), UART_CONSOLE_SIZE);
    assert_eq!(u.output()[0], b'o');
}

#[test]
fn uart_reads() {
    let mut u = uart_com1::new();
    assert_eq!(u.pio_read(UART_LSR, 1), 0x60);
    assert_eq!(u.pio_read(0x3f9, 1), 0);
    assert_eq!(u.pio_read(0x3f8, 1), 0);
    assert_eq!(u.pio_read(0x80, 1), 0xff);
    assert_eq!(u.pio_read(0x80, 2), 0xffff);
    assert_eq!(u.pio_read(0x2f8, 4), 0xffff_ffff);
}

#[test]
fn out_writes_console_and_skips() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RAX] = GARBAGE | b'H' as u64; // only AL is the operand
    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 1);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(1, false, false, false, false, 0x3f8));
    let act = handle_io(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(act, exit_action::Resume);
    assert_eq!(u.output(), b"H");
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1001);
    assert_eq!(regs[VCPU_REGS_RAX], GARBAGE | b'H' as u64, "OUT must not modify RAX");
}

#[test]
fn in_byte_and_word_merge_into_rax() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RAX] = 0x1122_3344_5566_7788;
    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 1);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(1, true, false, false, false, UART_LSR));
    assert_eq!(handle_io(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Resume);
    assert_eq!(regs[VCPU_REGS_RAX], 0x1122_3344_5566_7760); // AL only
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1001);

    // inw from an unmapped port: AX = 0xffff, upper bits kept
    regs[VCPU_REGS_RAX] = 0x1122_3344_5566_7788;
    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 2);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(2, true, false, false, true, 0x80));
    handle_io(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(regs[VCPU_REGS_RAX], 0x1122_3344_5566_ffff);
}

#[test]
fn in_dword_zero_extends_rax() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RAX] = GARBAGE | 0x1234;
    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 1);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(4, true, false, false, false, 0x80));
    handle_io(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(regs[VCPU_REGS_RAX], 0xffff_ffff);
}

#[test]
fn string_io_goes_to_userspace_without_skipping() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 2);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(1, false, true, true, false, 0x3f8));
    let act = handle_io(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(act, exit_action::ExitToUser(EXIT_REASON_IO_INSTRUCTION));
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    assert!(u.output().is_empty());
}

#[test]
fn cpuid_passthrough_zero_extends_and_skips() {
    let mut u = uart_com1::new();
    let mut regs = [GARBAGE; 16];
    regs[VCPU_REGS_RAX] = GARBAGE | 7;
    regs[VCPU_REGS_RCX] = GARBAGE | 3; // subleaf
    let mut v = exit(EXIT_REASON_CPUID, 2);
    let act = handle_cpuid(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(act, exit_action::Resume);
    assert_eq!(regs[VCPU_REGS_RAX], 3); // our fake echoes the subleaf in EAX
    assert_eq!(regs[VCPU_REGS_RBX], 0xaaaa);
    assert_eq!(regs[VCPU_REGS_RCX], 0xbbbb);
    assert_eq!(regs[VCPU_REGS_RDX], 0xcccc);
    assert_eq!(regs[VCPU_REGS_RSI], GARBAGE, "other registers untouched");
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1002);
}

#[test]
fn cpuid_leaf0_is_the_host_vendor() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    let mut v = exit(EXIT_REASON_CPUID, 2);
    handle_cpuid(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(regs[VCPU_REGS_RBX], 0x756e_6547);
    assert_eq!(regs[VCPU_REGS_RDX], 0x4965_6e69);
    assert_eq!(regs[VCPU_REGS_RCX], 0x6c65_746e);
}

#[test]
fn cpuid_leaf1_hides_vmx_and_sets_hypervisor_bit() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RAX] = 1;
    let mut v = exit(EXIT_REASON_CPUID, 2);
    handle_cpuid(&mut regs, &mut v, &mut ctx(&mut u));
    let ecx = regs[VCPU_REGS_RCX] as u32;
    assert_ne!(ecx & CPUID_1_ECX_HYPERVISOR, 0);
    assert_eq!(ecx & CPUID_1_ECX_VMX, 0);
    // everything else is the host's
    assert_eq!(ecx, (0x7ffa_3203 | CPUID_1_ECX_HYPERVISOR) & !CPUID_1_ECX_VMX);
    assert_eq!(regs[VCPU_REGS_RAX], 0x000906ea);
    assert_eq!(regs[VCPU_REGS_RDX], 0xbfeb_fbff);
}

#[test]
fn cpuid_hypervisor_signature() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RAX] = 0x4000_0000;
    let mut v = exit(EXIT_REASON_CPUID, 2);
    handle_cpuid(&mut regs, &mut v, &mut ctx(&mut u));
    assert_eq!(regs[VCPU_REGS_RAX], 0x4000_0001);
    let mut sig = [0u8; 12];
    sig[0..4].copy_from_slice(&(regs[VCPU_REGS_RBX] as u32).to_le_bytes());
    sig[4..8].copy_from_slice(&(regs[VCPU_REGS_RCX] as u32).to_le_bytes());
    sig[8..12].copy_from_slice(&(regs[VCPU_REGS_RDX] as u32).to_le_bytes());
    assert_eq!(&sig, b"KtVmxKtVmx\0\0");
}

#[test]
fn rdmsr_reads_guest_state_into_edx_eax() {
    let mut regs = [GARBAGE; 16];
    regs[VCPU_REGS_RCX] = MSR_FS_BASE as u64;
    let mut v = exit(EXIT_REASON_MSR_READ, 2);
    v.vmcs_writel(GUEST_FS_BASE, 0x0000_7fff_dead_b000);
    assert_eq!(handle_rdmsr(&mut regs, &mut v), exit_action::Resume);
    assert_eq!(regs[VCPU_REGS_RAX], 0xdead_b000);
    assert_eq!(regs[VCPU_REGS_RDX], 0x7fff);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1002);

    regs[VCPU_REGS_RCX] = MSR_EFER as u64;
    v.vmcs_writel(GUEST_IA32_EFER, EFER_LME | EFER_LMA | EFER_SCE);
    handle_rdmsr(&mut regs, &mut v);
    assert_eq!(regs[VCPU_REGS_RAX], EFER_LME | EFER_LMA | EFER_SCE);
    assert_eq!(regs[VCPU_REGS_RDX], 0);
}

#[test]
fn msr_field_mapping_is_complete() {
    let map = [
        (MSR_EFER, GUEST_IA32_EFER),
        (MSR_IA32_SYSENTER_CS, GUEST_SYSENTER_CS),
        (MSR_IA32_SYSENTER_ESP, GUEST_SYSENTER_ESP),
        (MSR_IA32_SYSENTER_EIP, GUEST_SYSENTER_EIP),
        (MSR_FS_BASE, GUEST_FS_BASE),
        (MSR_GS_BASE, GUEST_GS_BASE),
        (MSR_IA32_CR_PAT, GUEST_IA32_PAT),
        (MSR_IA32_DEBUGCTLMSR, GUEST_IA32_DEBUGCTL),
    ];
    for (i, (msr, field)) in map.iter().enumerate() {
        let mut v = MockVmcs::default();
        let val = if *msr == MSR_EFER { EFER_NX } else { 0x100 + i as u64 };
        assert!(vmx_set_msr(&mut v, *msr, val), "msr {msr:#x}");
        assert_eq!(v.vmcs_readl(*field), val, "msr {msr:#x} -> field {field:#x}");
        assert_eq!(vmx_get_msr(&v, *msr), Some(val));
    }
}

#[test]
fn unknown_msr_injects_gp_and_does_not_skip() {
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RCX] = 0x1234_5678;
    let mut v = exit(EXIT_REASON_MSR_READ, 2);
    assert_eq!(handle_rdmsr(&mut regs, &mut v), exit_action::InjectGp);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    let mut v = exit(EXIT_REASON_MSR_WRITE, 2);
    assert_eq!(handle_wrmsr(&mut regs, &mut v), exit_action::InjectGp);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    // KERNEL_GS_BASE is real but not in the VMCS: not supported by this stage.
    assert_eq!(vmx_get_msr(&v, MSR_KERNEL_GS_BASE), None);
}

#[test]
fn wrmsr_combines_edx_eax() {
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RCX] = MSR_GS_BASE as u64;
    regs[VCPU_REGS_RAX] = GARBAGE | 0x1111_2222;
    regs[VCPU_REGS_RDX] = 0x0000_0000_0000_7fff;
    let mut v = exit(EXIT_REASON_MSR_WRITE, 2);
    assert_eq!(handle_wrmsr(&mut regs, &mut v), exit_action::Resume);
    assert_eq!(v.vmcs_readl(GUEST_GS_BASE), 0x7fff_1111_2222);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1002);
}

#[test]
fn wrmsr_efer_rejects_reserved_bits() {
    let mut regs = [0u64; 16];
    regs[VCPU_REGS_RCX] = MSR_EFER as u64;
    regs[VCPU_REGS_RAX] = EFER_LME | (1 << 20); // bit 20 is reserved
    let mut v = exit(EXIT_REASON_MSR_WRITE, 2);
    assert_eq!(handle_wrmsr(&mut regs, &mut v), exit_action::InjectGp);
    assert_eq!(v.vmcs_readl(GUEST_IA32_EFER), 0, "a rejected write changes nothing");
    regs[VCPU_REGS_RAX] = EFER_LME | EFER_NX;
    assert_eq!(handle_wrmsr(&mut regs, &mut v), exit_action::Resume);
    assert_eq!(v.vmcs_readl(GUEST_IA32_EFER), EFER_LME | EFER_NX);
}

#[test]
fn hlt_is_skipped_and_halts() {
    let mut v = exit(EXIT_REASON_HLT, 1);
    assert_eq!(handle_hlt(&mut v), exit_action::Halt);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1001);
}

#[test]
fn basic_reason_masks_flags() {
    assert_eq!(basic_exit_reason(EXIT_REASON_IO_INSTRUCTION), 30);
    assert_eq!(basic_exit_reason(0x8000_0021), 33);
    assert_eq!(basic_exit_reason(12 | 1 << 27), 12);
}

#[test]
fn dispatch_routes_each_reason() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];

    let mut v = exit(EXIT_REASON_HLT, 1);
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Halt);

    let mut v = exit(EXIT_REASON_CPUID, 2);
    regs[VCPU_REGS_RAX] = 0x4000_0000;
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Resume);
    assert_eq!(regs[VCPU_REGS_RAX], 0x4000_0001);

    let mut v = exit(EXIT_REASON_IO_INSTRUCTION, 1);
    v.vmcs_writel(EXIT_QUALIFICATION, io_qual(1, false, false, false, false, 0x3f8));
    regs[VCPU_REGS_RAX] = b'Z' as u64;
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Resume);
    assert_eq!(u.output(), b"Z");

    let mut v = exit(EXIT_REASON_MSR_READ, 2);
    regs[VCPU_REGS_RCX] = 0xbad;
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::InjectGp);

    let mut v = exit(EXIT_REASON_MSR_WRITE, 2);
    regs[VCPU_REGS_RCX] = MSR_FS_BASE as u64;
    regs[VCPU_REGS_RAX] = 5;
    regs[VCPU_REGS_RDX] = 0;
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Resume);
    assert_eq!(v.vmcs_readl(GUEST_FS_BASE), 5);
}

#[test]
fn dispatch_ignores_flag_bits_in_the_reason() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    let mut v = exit(EXIT_REASON_HLT | 1 << 27, 1);
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Halt);
}

#[test]
fn dispatch_unknown_reasons_leave_the_loop() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    for r in [EXIT_REASON_EPT_VIOLATION, EXIT_REASON_TRIPLE_FAULT, EXIT_REASON_PREEMPTION_TIMER, EXIT_REASON_VMCALL] {
        let mut v = exit(r, 3);
        assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::ExitToUser(r));
        assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000, "an unhandled exit must not move RIP");
    }
}

#[test]
fn dispatch_interrupt_exits_just_resume() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    for r in [EXIT_REASON_EXTERNAL_INTERRUPT, EXIT_REASON_INTERRUPT_WINDOW] {
        let mut v = exit(r, 0);
        assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Resume);
        assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    }
}

#[test]
fn dispatch_failed_vm_entry() {
    let mut u = uart_com1::new();
    let mut regs = [0u64; 16];
    let raw = VMX_EXIT_REASONS_FAILED_VMENTRY | EXIT_REASON_INVALID_STATE;
    let mut v = exit(raw, 0);
    assert_eq!(vmx_handle_exit(&mut regs, &mut v, &mut ctx(&mut u)), exit_action::Fail(raw));
}
