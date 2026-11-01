// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 10: the pages behind the I/O and MSR bitmaps, and the VMCS
//! wiring that makes the CPU consult them. NOT COMPILED by the course author (no
//! Rust-enabled kernel was available): fix small API mismatches against
//! ~/WS/linux_mainline/rust once, and say so.
//!
//! The bit math is in src/vmx_bitmaps.rs (host-tested). Here you only allocate and point.
//! Book: SDM Vol. 3C 25.6.4 (I/O bitmaps), 25.6.9 (MSR bitmaps), Table 25-5/25-6.
//! Kernel: vmx.c `vmx_vcpu_create` (the `vmx->vmcs01.msr_bitmap` page and the
//! `vmx_disable_intercept_for_msr` list), `init_vmcs` (`vmcs_write64(MSR_BITMAP, ...)`).

use crate::hw::PageBox;
use crate::vmcs_ops::VmcsOps;
use crate::vmx::*;
use crate::vmx_bitmaps::*;
use kernel::prelude::*;

/// The three 4 KiB pages. Allocation is PROVIDED.
pub(crate) struct IntercepBitmaps {
    pub(crate) io_a: PageBox,
    pub(crate) io_b: PageBox,
    pub(crate) msr: PageBox,
}

impl IntercepBitmaps {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self { io_a: PageBox::new()?, io_b: PageBox::new()?, msr: PageBox::new()? })
    }

    /// Program the bitmaps and the VMCS:
    ///  1. every I/O port intercepted (fill A and B with 0xff), then pass port 0x80
    ///     through (`io_bitmap_set(.., 0x80, 1, false)`);
    ///  2. MSR bitmap = all intercepted (`msr_bitmap_init_all_intercept`);
    ///  3. write the three *physical* addresses (`PageBox::phys()`) to IO_BITMAP_A,
    ///     IO_BITMAP_B and MSR_BITMAP;
    ///  4. set CPU_BASED_USE_IO_BITMAPS | CPU_BASED_USE_MSR_BITMAPS in
    ///     CPU_BASED_VM_EXEC_CONTROL and clear CPU_BASED_UNCOND_IO_EXITING.
    ///     Careful: stage 4's `setup_vmcs_config` decides which bits are *allowed*; use
    ///     `vmcs_set_bits`/`vmcs_clear_bits`, do not overwrite the whole control word.
    /// Forgetting step 4 is the classic bug: the pointers are ignored and *every*
    /// MSR access / I/O still behaves as before (or, with the bit set but a zero
    /// pointer, the VM entry fails with VMXERR_ENTRY_INVALID_CONTROL_FIELD).
    pub(crate) fn vmx_setup_bitmaps<V: VmcsOps>(&mut self, vmcs: &mut V) -> Result {
        let _ = vmcs;
        // TODO(stage 10): delete this line and implement the four steps above.
        // (Returns an error instead of todo!() so an unfinished stage reports FAIL in
        // the selftest instead of panicking the kernel.)
        Err(ENOSYS)
    }
}

/// Stop intercepting `msr` for reads (convenience over `vmx_disable_intercept_for_msr`
/// on the page's slice).
pub(crate) fn msr_passthrough_read(bm: &mut IntercepBitmaps, msr: u32) {
    vmx_disable_intercept_for_msr(bm.msr.as_mut_slice(), msr, MSR_TYPE_R);
}

// ---- self test -------------------------------------------------------------------

/// The raw facts the selftest needs about one VM exit.
pub(crate) struct RawExit {
    pub(crate) reason: u32,
    pub(crate) qualification: u64,
    pub(crate) rax: u64,
    pub(crate) rcx: u64,
}

/// WIRING (you write this once): the selftest needs three things from your stages 3-7.
/// Implement them on your own vcpu type in `Wired` below. Nothing else is assumed.
pub(crate) trait GuestUnderTest: Sized {
    type Vmcs: VmcsOps;
    /// A real-mode guest (CS=DS=ES=SS base 0, unrestricted guest, RIP=0x1000) with
    /// `code` copied to guest-physical 0x1000 and nothing else mapped that it needs.
    fn start(code: &[u8]) -> Result<Self>;
    /// The vcpu's VMCS, current on this CPU.
    fn vmcs(&mut self) -> &mut Self::Vmcs;
    /// VMLAUNCH/VMRESUME until the next VM exit, return its raw facts. No handling.
    fn run_until_exit(&mut self) -> core::result::Result<RawExit, &'static str>;
}

/// Replace with `type Wired = YourVcpuType;` once it implements `GuestUnderTest`.
pub(crate) struct Wired;
/// Placeholder VMCS type so this file is self-contained: replace with your stage 3 type.
pub(crate) struct NoVmcs;
impl VmcsOps for NoVmcs {
    fn vmcs_readl(&self, _field: u32) -> u64 {
        todo!("stage 10: use your stage 3 real VMCS type here")
    }
    fn vmcs_writel(&mut self, _field: u32, _value: u64) {
        todo!("stage 10")
    }
}
impl GuestUnderTest for Wired {
    type Vmcs = NoVmcs;
    fn start(_code: &[u8]) -> Result<Self> {
        // TODO(stage 10): wire to your stage 6/7 vcpu, then return Ok(..).
        Err(ENOSYS)
    }
    fn vmcs(&mut self) -> &mut Self::Vmcs {
        todo!("stage 10")
    }
    fn run_until_exit(&mut self) -> core::result::Result<RawExit, &'static str> {
        todo!("stage 10")
    }
}

// guest/10_bitmaps.S, assembled (`make -C guest 10_bitmaps.rs`).
const GUEST_CODE: [u8; 27] = [
    0xb0, 0x00, 0xe6, 0x80, 0xb0, 0x41, 0xba, 0xf8, 0x03, 0xee, 0x66, 0xb9, 0x74, 0x01, 0x00,
    0x00, 0x0f, 0x32, 0x66, 0xb9, 0x80, 0x00, 0x00, 0xc0, 0x0f, 0x32, 0xf4,
];

fn skip_instruction<V: VmcsOps>(v: &mut V) {
    let len = v.vmcs_read32(VM_EXIT_INSTRUCTION_LEN) as u64;
    let rip = v.vmcs_readl(GUEST_RIP);
    v.vmcs_writel(GUEST_RIP, rip + len);
}

pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let mut g = Wired::start(&GUEST_CODE).map_err(|_| "cannot create guest")?;
    let mut bm = IntercepBitmaps::new().map_err(|_| "cannot allocate bitmaps")?;
    bm.vmx_setup_bitmaps(g.vmcs()).map_err(|_| "vmx_setup_bitmaps failed")?;
    msr_passthrough_read(&mut bm, MSR_IA32_SYSENTER_CS);

    // 1. `out 0x80` must NOT exit; the first exit is `out %al, %dx` with dx = 0x3f8.
    let e = g.run_until_exit()?;
    if e.reason != EXIT_REASON_IO_INSTRUCTION {
        return Err("first exit is not an I/O exit: port 0x80 was intercepted, or the bitmaps are not in use");
    }
    if (e.qualification >> 16) as u16 != 0x3f8 || e.rax as u8 != 0x41 {
        return Err("I/O exit has the wrong port/value (did out 0x80 exit instead?)");
    }
    skip_instruction(g.vmcs());

    // 2. `rdmsr 0x174` must NOT exit; the next exit is `rdmsr 0xc0000080`.
    let e = g.run_until_exit()?;
    if e.reason != EXIT_REASON_MSR_READ {
        return Err("expected an MSR-read exit");
    }
    if e.rcx as u32 != 0xc000_0080 {
        return Err("rdmsr of IA32_SYSENTER_CS exited although it was passed through");
    }
    skip_instruction(g.vmcs());

    // 3. then hlt.
    let e = g.run_until_exit()?;
    if e.reason != EXIT_REASON_HLT {
        return Err("expected hlt");
    }
    Ok(())
}
