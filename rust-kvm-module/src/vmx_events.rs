//! Stage 9: events: injecting interrupts and exceptions into the guest, and being
//! interrupted yourself. Kernel counterparts: arch/x86/kvm/vmx/vmx.c (`vmx_inject_irq`,
//! `vmx_inject_nmi`, `vmx_inject_exception`, `__vmx_complete_interrupts`,
//! `vmx_interrupt_blocked`, `vmx_nmi_blocked`, `vmx_enable_irq_window`,
//! `vmx_update_exception_bitmap`, `vmx_set_hv_timer`), arch/x86/kvm/vmx/vmx.h
//! (`is_exception_n`, `is_page_fault`), arch/x86/kvm/x86.c (`kvm_inject_realmode_interrupt`).
//! Book: Intel SDM Vol. 3C: 25.8.3 / Table 25-17 "VM-entry interruption-information
//! field" (event injection), 27.2.2 "information for VM exits due to vectored events"
//! (Table 27-15 exit interruption information, Table 27-16 IDT-vectoring information),
//! 25.4.2 "guest non-register state" (interruptibility), 25.5.1 / 27.2.x (window
//! exiting), 25.5.1 (exception bitmap), 25.5.1 + Appendix A.6 (VMX-preemption timer,
//! IA32_VMX_MISC[4:0]).
//!
//! Direction 1 (host -> guest): write the event into `VM_ENTRY_INTR_INFO_FIELD` before
//! VMLAUNCH/VMRESUME; the CPU delivers it through the guest IDT/IVT as if it had
//! happened in the guest, then clears the valid bit again.
//! Direction 2 (guest -> host): external interrupts, NMIs, selected exceptions and the
//! preemption timer cause VM exits; if one hit while the CPU was delivering an event
//! (`IDT_VECTORING_INFO_FIELD`), that event must be re-injected or it is lost.
#![allow(unused_variables, unused_imports)]

use crate::vmcs_ops::VmcsOps;
use crate::vmx::*;
// Interruptibility-state bits, defined in stage 7.
use crate::vmx_exit::{GUEST_INTR_STATE_MOV_SS, GUEST_INTR_STATE_NMI, GUEST_INTR_STATE_STI};

pub const NMI_VECTOR: u8 = 2;
pub const OF_VECTOR: u8 = 4;

/// Bit 11 of an interruption-information / IDT-vectoring field: "deliver error code".
pub const INTR_INFO_DELIVER_CODE: u32 = INTR_INFO_DELIVER_CODE_MASK;
/// Bit 12 of the exit interruption information: "NMI unblocking due to IRET".
pub const INTR_INFO_UNBLOCK_NMI: u32 = 1 << 12;

/// Does the exception with this vector push an error code? #DF (8), #TS (10), #NP (11),
/// #SS (12), #GP (13), #PF (14), #AC (17). (Real CPUs also push one for #CP (21, CET);
/// ignore it.) SDM Vol. 3A 6.15 table of exceptions. Used for the "deliver error
/// code" bit: setting it for any other vector makes VM entry fail.
pub fn exception_has_error_code(vector: u8) -> bool {
    todo!("8, 10..=14, 17")
}

/// Inject an external (maskable) interrupt `vector` on the next VM entry:
/// `VM_ENTRY_INTR_INFO_FIELD = vector | INTR_TYPE_EXT_INTR | INTR_INFO_VALID_MASK`.
/// No error code, no instruction length. (kernel: `vmx_inject_irq`, for the non-soft case.)
pub fn vmx_inject_irq(vmcs: &mut impl VmcsOps, vector: u8) {
    todo!("vector | EXT_INTR | VALID")
}

/// Inject an NMI (type 2, vector 2, valid). (kernel: `vmx_inject_nmi`.)
pub fn vmx_inject_nmi(vmcs: &mut impl VmcsOps) {
    todo!("NMI type, vector 2")
}

/// Inject a software interrupt (`INT n`): type `INTR_TYPE_SOFT_INTR`. Software events
/// need the length of the instruction that "caused" them so the pushed return address
/// is right: write `inst_len` to `VM_ENTRY_INSTRUCTION_LEN`. (kernel: `vmx_inject_irq`
/// with `vcpu->arch.interrupt.soft`.)
pub fn vmx_inject_soft_intr(vmcs: &mut impl VmcsOps, vector: u8, inst_len: u32) {
    todo!("SOFT_INTR type + instruction length")
}

/// Inject an exception. Type is `INTR_TYPE_HARD_EXCEPTION`, except #BP (3) and #OF (4),
/// which are traps raised by an instruction (`int3`, `into`): `INTR_TYPE_SOFT_EXCEPTION`
/// and they also need `VM_ENTRY_INSTRUCTION_LEN = inst_len`. If `error_code` is `Some(c)`
/// *and* `exception_has_error_code(vector)`, set the deliver-code bit (bit 11) and write
/// `c` to `VM_ENTRY_EXCEPTION_ERROR_CODE`; otherwise leave the bit clear (a `Some` for a
/// vector that takes none is ignored: the hardware would refuse the entry).
/// (kernel: `vmx_inject_exception`.)
pub fn vmx_inject_exception(vmcs: &mut impl VmcsOps, vector: u8, error_code: Option<u32>, inst_len: u32) {
    todo!("pick the type, error code and length")
}

/// A decoded interruption-information word (VM-exit interruption information, Table 27-15;
/// same layout as the entry field and, for the low bits, the IDT-vectoring field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct intr_info {
    /// Bit 31.
    pub valid: bool,
    /// Bits 7:0.
    pub vector: u8,
    /// Bits 10:8 *in place* (e.g. `INTR_TYPE_HARD_EXCEPTION` = 3 << 8), so it can be compared
    /// with the `INTR_TYPE_*` constants directly.
    pub itype: u32,
    /// Bit 11: an error code was pushed (it is in `VM_EXIT_INTR_ERROR_CODE`).
    pub has_error_code: bool,
    /// Bit 12 (exit information only): this NMI exit happened while blocked by IRET.
    pub nmi_unblocking: bool,
}

pub fn decode_intr_info(info: u32) -> intr_info {
    todo!("split the bits")
}

/// Read and decode `VM_EXIT_INTR_INFO` (kernel: `vmx_get_intr_info`).
pub fn vmx_exit_intr_info(vmcs: &impl VmcsOps) -> intr_info {
    todo!("read VM_EXIT_INTR_INFO and decode")
}

/// Is `info` a valid *hardware exception* with this vector? (kernel: `is_exception_n`:
/// `(info & (VALID | TYPE_MASK | VECTOR_MASK)) == (VALID | HARD_EXCEPTION | vector)`.)
pub fn is_exception_n(info: u32, vector: u8) -> bool {
    todo!("mask and compare")
}

/// After a VM exit: was an event being delivered when it happened? If
/// `IDT_VECTORING_INFO_FIELD` has bit 31 set, the interrupted event must be injected again
/// on the next entry, so copy it to `VM_ENTRY_INTR_INFO_FIELD` and return `true`:
/// - keep valid, type, vector and the error-code bit; clear bit 12 (undefined in the
///   IDT-vectoring field, reserved in the entry field);
/// - if the error-code bit is set, copy `IDT_VECTORING_ERROR_CODE` to
///   `VM_ENTRY_EXCEPTION_ERROR_CODE`;
/// - for software events (types `INTR_TYPE_SOFT_INTR`, `INTR_TYPE_SOFT_EXCEPTION`,
///   `INTR_TYPE_PRIV_SW_EXCEPTION`) copy `VM_EXIT_INSTRUCTION_LEN` to
///   `VM_ENTRY_INSTRUCTION_LEN`.
/// If bit 31 is clear, touch nothing and return `false`.
/// (kernel: `__vmx_complete_interrupts`, which instead requeues into KVM's event queues so
/// the event can also be *changed*, e.g. an exception turned into #DF; this simple
/// version re-injects verbatim. NMIs also need the "NMI blocking" bookkeeping the kernel
/// does with `vmx_set_nmi_mask`: ignored here.)
pub fn vmx_complete_interrupts(vmcs: &mut impl VmcsOps) -> bool {
    todo!("copy IDT_VECTORING_INFO into the entry fields")
}

/// Can an external interrupt be injected right now (kernel: `!vmx_interrupt_blocked`)?
/// Yes iff `GUEST_RFLAGS.IF` is set, `GUEST_INTERRUPTIBILITY_INFO` has neither the
/// blocking-by-STI nor the blocking-by-MOV-SS bit, and no other event is already queued
/// (bit 31 of `VM_ENTRY_INTR_INFO_FIELD`: only one event fits in one VM entry).
pub fn vmx_interrupt_allowed(vmcs: &impl VmcsOps) -> bool {
    todo!("IF, interruptibility, nothing already queued")
}

/// Can an NMI be injected right now (kernel: `!vmx_nmi_blocked`)? Not if the guest is in
/// NMI-blocking state (interruptibility bit 3), or blocked by STI/MOV-SS (bits 0, 1), or
/// another event is already queued for this entry.
pub fn vmx_nmi_allowed(vmcs: &impl VmcsOps) -> bool {
    todo!("interruptibility bits 0,1,3 and queued event")
}

/// Ask for a VM exit as soon as the guest can take an interrupt: set
/// `CPU_BASED_INTR_WINDOW_EXITING` in `CPU_BASED_VM_EXEC_CONTROL`, other bits untouched.
/// Use it when `vmx_interrupt_allowed()` is false but an interrupt is pending; the exit
/// (`EXIT_REASON_INTERRUPT_WINDOW`) tells you the window is open. (kernel: `vmx_enable_irq_window`.)
pub fn vmx_enable_irq_window(vmcs: &mut impl VmcsOps) {
    todo!("set CPU_BASED_INTR_WINDOW_EXITING")
}

/// Undo `vmx_enable_irq_window` (clear the bit; once the exit has fired, keeping it set
/// would exit again and again).
pub fn vmx_disable_irq_window(vmcs: &mut impl VmcsOps) {
    todo!("clear CPU_BASED_INTR_WINDOW_EXITING")
}

/// Intercept (`true`) or stop intercepting (`false`) exception `vector` (0..=31): set or
/// clear bit `vector` of `EXCEPTION_BITMAP`; other bits untouched. An intercepted
/// exception that the guest raises causes `EXIT_REASON_EXCEPTION_NMI` instead of being
/// delivered through the guest IDT. (kernel: `vmx_update_exception_bitmap`.)
pub fn vmx_update_exception_bitmap(vmcs: &mut impl VmcsOps, vector: u8, intercept: bool) {
    todo!("set or clear bit `vector`")
}

/// Is `vector` currently intercepted?
pub fn vmx_exception_intercepted(vmcs: &impl VmcsOps, vector: u8) -> bool {
    todo!("test the bit")
}

/// Convert `ns` nanoseconds into a VMX-preemption-timer value. The timer counts down at the
/// TSC rate divided by 2^X, where X = `IA32_VMX_MISC` bits 4:0 (`misc`).
/// `tsc_cycles = ns * tsc_khz / 1_000_000` (do the multiplication in u128; it overflows u64),
/// `value = tsc_cycles >> X`, saturated to `u32::MAX` (the field is 32 bits).
/// (kernel: `vmx_set_hv_timer`: `delta_tsc >> cpu_preemption_timer_multi`.)
pub fn preemption_timer_value(ns: u64, tsc_khz: u64, misc: u64) -> u32 {
    todo!("ns*khz/1e6 >> (misc & 0x1f), saturate")
}

/// Arm the timer: write `value` to `VMX_PREEMPTION_TIMER_VALUE` and set
/// `PIN_BASED_VMX_PREEMPTION_TIMER` in `PIN_BASED_VM_EXEC_CONTROL`, other pin bits untouched.
/// The timer runs only in VMX non-root mode and exits with
/// `EXIT_REASON_PREEMPTION_TIMER` when it reaches 0. Unless the "save VMX-preemption
/// timer value" exit control is on, the field is not updated on exit: re-arm every entry.
pub fn vmx_arm_preemption_timer(vmcs: &mut impl VmcsOps, value: u32) {
    todo!("write value, set pin-based bit")
}

/// Disarm: clear `PIN_BASED_VMX_PREEMPTION_TIMER`, other bits untouched.
pub fn vmx_disarm_preemption_timer(vmcs: &mut impl VmcsOps) {
    todo!("clear pin-based bit")
}
