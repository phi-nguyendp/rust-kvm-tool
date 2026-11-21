//! Stage 7: handling VM exits. Kernel counterparts: arch/x86/kvm/vmx/vmx.c
//! (`__vmx_handle_exit`, `kvm_vmx_exit_handlers[]`, `handle_io`, `handle_wrmsr`,
//! `vmx_get_msr`/`vmx_set_msr`), arch/x86/kvm/cpuid.c (`kvm_emulate_cpuid`),
//! arch/x86/kvm/x86.c (`kvm_skip_emulated_instruction`).
//! Book: Intel SDM Vol. 3C, 27.2 (recording VM-exit information: exit reason,
//! exit qualification, instruction length), Table 27-5 (I/O exit qualification),
//! App. C (basic exit reasons). Vol. 3A for the UART: any 16550 datasheet.
//!
//! Design: a handler never touches hardware. It gets the guest's general purpose
//! registers (saved by the VM-exit stub, stage 6) and the current VMCS, and returns an
//! [`exit_action`] telling the run loop what to do next, like the `int` returned by
//! the kernel's handlers (1 = resume guest, 0 = go to userspace).
#![allow(unused_variables, unused_imports)]

use crate::vmcs_ops::VmcsOps;
use crate::vmx::*;

// Index into the saved guest register array, in the order the kernel uses
// (arch/x86/include/asm/kvm_vcpu_regs.h, VCPU_REGS_*).
pub const VCPU_REGS_RAX: usize = 0;
pub const VCPU_REGS_RCX: usize = 1;
pub const VCPU_REGS_RDX: usize = 2;
pub const VCPU_REGS_RBX: usize = 3;
pub const VCPU_REGS_RSP: usize = 4;
pub const VCPU_REGS_RBP: usize = 5;
pub const VCPU_REGS_RSI: usize = 6;
pub const VCPU_REGS_RDI: usize = 7;
pub const VCPU_REGS_R8: usize = 8;
pub const VCPU_REGS_R15: usize = 15;
pub type regs_t = [u64; 16];

// GUEST_INTERRUPTIBILITY_INFO bits (SDM 24.4.2, kernel: GUEST_INTR_STATE_*).
pub const GUEST_INTR_STATE_STI: u32 = 1 << 0;
pub const GUEST_INTR_STATE_MOV_SS: u32 = 1 << 1;
pub const GUEST_INTR_STATE_SMI: u32 = 1 << 2;
pub const GUEST_INTR_STATE_NMI: u32 = 1 << 3;

/// What the run loop does after a handler returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum exit_action {
    /// Handled in the kernel: VMRESUME the guest. (kernel: handler returned 1)
    Resume,
    /// Not ours to handle: leave the run loop, report this *basic* exit reason
    /// to the caller. (kernel: return 0 with `kvm_run->exit_reason` set)
    ExitToUser(u32),
    /// The guest executed HLT; the instruction has been skipped.
    Halt,
    /// Inject #GP(0) into the guest instead of completing the instruction
    /// (RIP is *not* advanced). Stage 9 does the injection.
    InjectGp,
    /// The VM entry itself failed; carries the raw `VM_EXIT_REASON` (bit 31 set).
    Fail(u32),
}

/// `cpuid` on the host CPU: (leaf, subleaf) -> [eax, ebx, ecx, edx].
/// In the module this is `hw::cpuid`; tests pass a fake.
pub type cpuid_fn = fn(u32, u32) -> [u32; 4];

/// Everything a handler needs besides registers and the VMCS.
pub struct exit_ctx<'a> {
    pub uart: &'a mut uart_com1,
    pub host_cpuid: cpuid_fn,
}

// ---------------------------------------------------------------------------------
// A minimal 16550 UART at COM1, just enough for `console=ttyS0`-style output.
// ---------------------------------------------------------------------------------

pub const UART_COM1_BASE: u16 = 0x3f8;
pub const UART_COM1_LAST: u16 = 0x3ff;
pub const UART_THR: u16 = 0x3f8; // transmit holding register (write)
pub const UART_LSR: u16 = 0x3fd; // line status register (read)
pub const UART_LSR_THRE_TEMT: u32 = 0x60; // transmitter empty: always ready
pub const UART_CONSOLE_SIZE: usize = 256;

pub struct uart_com1 {
    pub buf: [u8; UART_CONSOLE_SIZE],
    pub len: usize,
}

impl uart_com1 {
    /// PROVIDED.
    pub const fn new() -> Self {
        Self {
            buf: [0; UART_CONSOLE_SIZE],
            len: 0,
        }
    }

    /// PROVIDED: everything the guest has written to the transmit register so far.
    pub fn output(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// The guest executed `out port, val` with an operand of `size` bytes (1, 2 or 4).
    /// A write to `UART_THR` appends the *low byte* of `val` to `buf`; once the buffer is
    /// full, bytes are dropped. Writes to any other port (including the rest of the
    /// COM1 range) are ignored.
    pub fn pio_write(&mut self, port: u16, size: u8, val: u32) {
        if port == UART_THR && size == 1 {
            if self.len < UART_CONSOLE_SIZE {
                self.buf[self.len] = val as u8;
                self.len += 1;
            }
        }
    }

    /// The guest executed `in val, port`. Returns the value, *unmasked*:
    /// `UART_LSR` reads `UART_LSR_THRE_TEMT` (transmitter always ready); other COM1 ports
    /// read 0; a port outside `UART_COM1_BASE..=UART_COM1_LAST` is "no device" and
    /// reads as all ones for `size` bytes (0xff, 0xffff or 0xffff_ffff), like an
    /// unterminated ISA bus.
    pub fn pio_read(&mut self, port: u16, size: u8) -> u32 {
        if port == UART_LSR && size == 1 {
            UART_LSR_THRE_TEMT
        } else if port >= UART_COM1_BASE && port <= UART_COM1_LAST {
            0
        } else {
            match size {
                1 => 0xff,
                2 => 0xffff,
                4 => 0xffff_ffff,
                _ => unreachable!(),
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Common helpers
// ---------------------------------------------------------------------------------

/// Step the guest past the instruction that caused the exit.
/// `GUEST_RIP += VM_EXIT_INSTRUCTION_LEN`, and, because the instruction that was
/// "shadowing" interrupts (STI / MOV SS) has now completed, clear the STI and MOV-SS
/// bits of `GUEST_INTERRUPTIBILITY_INFO` (leave the SMI/NMI bits alone).
/// (kernel: `kvm_skip_emulated_instruction`, which calls `vmx_skip_emulated_instruction`
/// and `vmx_set_interrupt_shadow(vcpu, 0)`.) SDM 27.2.4: the length is only defined for
/// exits caused by instruction execution.
pub fn skip_emulated_instruction(vmcs: &mut impl VmcsOps) {
    vmcs.vmcs_write64(
        GUEST_RIP,
        vmcs.vmcs_read64(GUEST_RIP) + vmcs.vmcs_read64(VM_EXIT_INSTRUCTION_LEN),
    );
    let mut intr_state = vmcs.vmcs_read32(GUEST_INTERRUPTIBILITY_INFO);
    intr_state &= !(GUEST_INTR_STATE_STI | GUEST_INTR_STATE_MOV_SS);
    vmcs.vmcs_write32(GUEST_INTERRUPTIBILITY_INFO, intr_state);
}

// ---------------------------------------------------------------------------------
// CPUID
// ---------------------------------------------------------------------------------

/// First leaf of the hypervisor range; the "signature" leaf (see `kvm_cpuid`).
pub const HYPERV_CPUID_START: u32 = 0x4000_0000;

/// Emulate CPUID: leaf in EAX, subleaf in ECX; results go to EAX, EBX, ECX, EDX and,
/// as for any 32-bit register write, the upper halves of the 64-bit registers become 0.
/// Policy (kernel: `kvm_emulate_cpuid` + `__kvm_get_cpuid`):
/// - leaf 1: the host's values, but set ECX bit 31 (`CPUID_1_ECX_HYPERVISOR`, "I am
///   running under a hypervisor") and clear ECX bit 5 (`CPUID_1_ECX_VMX`: our guest may
///   not use VMX, we do not do nested virtualization);
/// - leaf `HYPERV_CPUID_START`: EAX = `0x4000_0001` (max hypervisor leaf), and the 12-byte
///   signature `b"KtVmxKtVmx\0\0"` in EBX:ECX:EDX (little-endian, 4 bytes per register);
/// - every other leaf: whatever `ctx.host_cpuid(leaf, subleaf)` says.
/// Then `skip_emulated_instruction`, and return `Resume`.
pub fn handle_cpuid(regs: &mut regs_t, vmcs: &mut impl VmcsOps, ctx: &mut exit_ctx) -> exit_action {
    let leaf = regs[VCPU_REGS_RAX] as u32;
    let subleaf = regs[VCPU_REGS_RCX] as u32;
    let mut result = (ctx.host_cpuid)(leaf, subleaf);
    if leaf == 1 {
        result[2] |= 1 << 31; // CPUID_1_ECX_HYPERVISOR
        result[2] &= !(1 << 5); // CPUID_1_ECX_VMX
    } else if leaf == HYPERV_CPUID_START {
        result[0] = HYPERV_CPUID_START + 1; // max hypervisor leaf
        result[1] = u32::from_le_bytes(*b"KtVm"); // EBX
        result[2] = u32::from_le_bytes(*b"xKtV"); // ECX
        result[3] = u32::from_le_bytes(*b"mx\0\0"); // EDX
    }
    regs[VCPU_REGS_RAX] = result[0] as u64;
    regs[VCPU_REGS_RBX] = result[1] as u64;
    regs[VCPU_REGS_RCX] = result[2] as u64;
    regs[VCPU_REGS_RDX] = result[3] as u64;
    skip_emulated_instruction(vmcs);
    exit_action::Resume
}

// ---------------------------------------------------------------------------------
// Port I/O
// ---------------------------------------------------------------------------------

/// Decoded exit qualification of an I/O instruction (SDM Table 27-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct io_exit {
    /// Access size in bytes: 1, 2 or 4.
    pub size: u8,
    /// true = IN/INS, false = OUT/OUTS.
    pub is_in: bool,
    /// String instruction (INS/OUTS).
    pub string: bool,
    /// REP prefix.
    pub rep: bool,
    /// Port number came from an immediate (IN al, imm8), not from DX.
    pub imm: bool,
    pub port: u16,
}

/// Bits of the I/O exit qualification: 2:0 = size - 1 (0, 1 or 3), 3 = direction
/// (0 = OUT, 1 = IN), 4 = string, 5 = REP, 6 = operand encoding (0 = DX, 1 = immediate),
/// 31:16 = port number. (kernel: `handle_io` uses `exit_qualification & 7`, `& 8`,
/// `& 16`, `>> 16`.)
pub fn decode_io_qualification(q: u64) -> io_exit {
    io_exit {
        size: ((q & 7) + 1) as u8,
        is_in: (q & 8) != 0,
        string: (q & 16) != 0,
        rep: (q & 32) != 0,
        imm: (q & 64) != 0,
        port: ((q >> 16) & 0xffff) as u16,
    }
}

/// Handle `EXIT_REASON_IO_INSTRUCTION` against the COM1 model.
/// - String instructions (INS/OUTS) are not emulated: return
///   `ExitToUser(EXIT_REASON_IO_INSTRUCTION)` and leave RIP alone.
/// - OUT: the value is RAX masked to `size` bytes; give it to `ctx.uart.pio_write`.
/// - IN: `ctx.uart.pio_read`; store into RAX as the CPU would: size 1 and 2 replace the
///   low 8/16 bits and keep the rest of RAX, size 4 zero-extends into the whole RAX.
/// - then `skip_emulated_instruction` and return `Resume`.
/// (kernel: `handle_io` -> `kvm_fast_pio` -> `emulator_pio_in/out`.)
pub fn handle_io(regs: &mut regs_t, vmcs: &mut impl VmcsOps, ctx: &mut exit_ctx) -> exit_action {
    let q = vmcs.vmcs_read64(EXIT_QUALIFICATION);
    let io = decode_io_qualification(q);
    if io.string {
        return exit_action::ExitToUser(basic_exit_reason(vmcs.vmcs_read32(VM_EXIT_REASON)));
    }
    if io.is_in {
        let val = ctx.uart.pio_read(io.port, io.size);
        match io.size {
            1 => regs[VCPU_REGS_RAX] = (regs[VCPU_REGS_RAX] & !0xff) | (val as u64 & 0xff),
            2 => regs[VCPU_REGS_RAX] = (regs[VCPU_REGS_RAX] & !0xffff) | (val as u64 & 0xffff),
            4 => regs[VCPU_REGS_RAX] = val as u64,
            _ => unreachable!(),
        }
    } else {
        let val = regs[VCPU_REGS_RAX] as u32;
        ctx.uart.pio_write(io.port, io.size, val);
    }
    skip_emulated_instruction(vmcs);
    exit_action::Resume
}

// ---------------------------------------------------------------------------------
// MSRs
// ---------------------------------------------------------------------------------

/// Read a guest MSR that lives in the VMCS (kernel: the VMCS-backed cases of
/// `vmx_get_msr`). Supported: EFER, SYSENTER_CS/ESP/EIP, FS_BASE, GS_BASE, CR_PAT and
/// IA32_DEBUGCTL, from their `GUEST_*` fields. Everything else (e.g. KERNEL_GS_BASE,
/// which lives in a separate save area, or any unknown MSR): `None`.
pub fn vmx_get_msr(vmcs: &impl VmcsOps, msr: u32) -> Option<u64> {
    match msr {
        MSR_EFER => Some(vmcs.vmcs_read64(GUEST_IA32_EFER)),
        MSR_IA32_SYSENTER_CS => Some(vmcs.vmcs_read64(GUEST_SYSENTER_CS)),
        MSR_IA32_SYSENTER_ESP => Some(vmcs.vmcs_read64(GUEST_SYSENTER_ESP)),
        MSR_IA32_SYSENTER_EIP => Some(vmcs.vmcs_read64(GUEST_SYSENTER_EIP)),
        MSR_FS_BASE => Some(vmcs.vmcs_read64(GUEST_FS_BASE)),
        MSR_GS_BASE => Some(vmcs.vmcs_read64(GUEST_GS_BASE)),
        MSR_IA32_CR_PAT => Some(vmcs.vmcs_read64(GUEST_IA32_PAT)),
        MSR_IA32_DEBUGCTLMSR => Some(vmcs.vmcs_read64(GUEST_IA32_DEBUGCTL)),
        _ => None,
    }
}

/// Write a guest MSR; the same set as `vmx_get_msr`. Returns false (the caller injects
/// #GP) for unsupported MSRs, and for EFER values with bits set outside
/// `EFER_SCE | EFER_LME | EFER_LMA | EFER_NX` (reserved bits, kernel: `kvm_valid_efer`).
pub fn vmx_set_msr(vmcs: &mut impl VmcsOps, msr: u32, val: u64) -> bool {
    match msr {
        MSR_EFER => {
            const EFER_MASK: u64 = EFER_SCE | EFER_LME | EFER_LMA | EFER_NX;
            if val & !EFER_MASK != 0 {
                return false;
            }
            vmcs.vmcs_write64(GUEST_IA32_EFER, val);
            true
        }
        MSR_IA32_SYSENTER_CS => {
            vmcs.vmcs_write64(GUEST_SYSENTER_CS, val);
            true
        }
        MSR_IA32_SYSENTER_ESP => {
            vmcs.vmcs_write64(GUEST_SYSENTER_ESP, val);
            true
        }
        MSR_IA32_SYSENTER_EIP => {
            vmcs.vmcs_write64(GUEST_SYSENTER_EIP, val);
            true
        }
        MSR_FS_BASE => {
            vmcs.vmcs_write64(GUEST_FS_BASE, val);
            true
        }
        MSR_GS_BASE => {
            vmcs.vmcs_write64(GUEST_GS_BASE, val);
            true
        }
        MSR_IA32_CR_PAT => {
            vmcs.vmcs_write64(GUEST_IA32_PAT, val);
            true
        }
        MSR_IA32_DEBUGCTLMSR => {
            vmcs.vmcs_write64(GUEST_IA32_DEBUGCTL, val);
            true
        }
        _ => false,
    }
}

/// RDMSR: MSR index in ECX; on success EDX:EAX = value (upper halves of RAX/RDX
/// zeroed), skip the instruction, `Resume`. Unsupported MSR: `InjectGp`, RIP unchanged.
/// (kernel: `kvm_emulate_rdmsr`.)
pub fn handle_rdmsr(regs: &mut regs_t, vmcs: &mut impl VmcsOps) -> exit_action {
    let msr = regs[VCPU_REGS_RCX] as u32;
    if let Some(val) = vmx_get_msr(vmcs, msr) {
        regs[VCPU_REGS_RAX] = val & 0xffff_ffff;
        regs[VCPU_REGS_RDX] = val >> 32;
        skip_emulated_instruction(vmcs);
        exit_action::Resume
    } else {
        exit_action::InjectGp
    }
}

/// WRMSR: MSR index in ECX, value in EDX:EAX. Success: skip + `Resume`; unsupported
/// MSR or invalid value: `InjectGp`, RIP unchanged. (kernel: `kvm_emulate_wrmsr`.)
pub fn handle_wrmsr(regs: &mut regs_t, vmcs: &mut impl VmcsOps) -> exit_action {
    let msr = regs[VCPU_REGS_RCX] as u32;
    let val = (regs[VCPU_REGS_RDX] << 32) | (regs[VCPU_REGS_RAX] & 0xffff_ffff);
    if vmx_set_msr(vmcs, msr, val) {
        skip_emulated_instruction(vmcs);
        exit_action::Resume
    } else {
        exit_action::InjectGp
    }
}

/// HLT: skip it and report `Halt` (a real VMM would block until an interrupt is
/// pending; ours stops the run loop). (kernel: `kvm_emulate_halt`.)
pub fn handle_hlt(vmcs: &mut impl VmcsOps) -> exit_action {
    skip_emulated_instruction(vmcs);
    exit_action::Halt
}

// ---------------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------------

/// Basic exit reason = `VM_EXIT_REASON` bits 15:0 (SDM 27.2.1; the upper bits hold flags
/// such as bit 31 "VM-entry failure").
pub fn basic_exit_reason(reason: u32) -> u32 {
    reason & 0xffff
}

/// The exit dispatcher (kernel: `__vmx_handle_exit` indexing `kvm_vmx_exit_handlers[]`).
/// Reads `VM_EXIT_REASON` and:
/// - bit 31 set (the VM *entry* failed; hardware did not run the guest): `Fail(raw reason)`;
/// - `CPUID`, `HLT`, `IO_INSTRUCTION`, `MSR_READ`, `MSR_WRITE`: the handlers above;
/// - `EXTERNAL_INTERRUPT`: `Resume`. The host interrupt is delivered by the run loop
///   after it re-enables interrupts (stage 9); there is nothing to emulate;
/// - `INTERRUPT_WINDOW`: `Resume` (stage 9 uses it);
/// - anything else: `ExitToUser(basic reason)`. An unknown exit must never silently
///   resume: the guest would just re-execute the same instruction forever.
pub fn vmx_handle_exit(
    regs: &mut regs_t,
    vmcs: &mut impl VmcsOps,
    ctx: &mut exit_ctx,
) -> exit_action {
    let reason = vmcs.vmcs_read32(VM_EXIT_REASON);
    if reason & (1 << 31) != 0 {
        return exit_action::Fail(reason);
    }
    match basic_exit_reason(reason) {
        EXIT_REASON_CPUID => handle_cpuid(regs, vmcs, ctx),
        EXIT_REASON_HLT => handle_hlt(vmcs),
        EXIT_REASON_IO_INSTRUCTION => handle_io(regs, vmcs, ctx),
        EXIT_REASON_MSR_READ => handle_rdmsr(regs, vmcs),
        EXIT_REASON_MSR_WRITE => handle_wrmsr(regs, vmcs),
        EXIT_REASON_EXTERNAL_INTERRUPT => exit_action::Resume,
        EXIT_REASON_INTERRUPT_WINDOW => exit_action::Resume,
        _ => exit_action::ExitToUser(basic_exit_reason(reason)),
    }
}
