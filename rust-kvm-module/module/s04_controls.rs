// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 04: run `setup_vmcs_config()` (src/vmx_controls.rs) against the
//! real capability MSRs. Nothing to implement here: all logic is in the shared file.
//! Kernel counterpart: arch/x86/kvm/vmx/vmx.c `setup_vmcs_config()` / `hardware_setup()`.
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).

use kernel::prelude::*;

use crate::{s01_caps::HwMsrs, vmx::*, vmx_controls::*};

/// The machine-wide control selection, from the real MSRs. (Needs VMX to exist: stage 1.)
pub(crate) fn hw_vmcs_config() -> core::result::Result<vmcs_config, &'static str> {
    setup_vmcs_config(&HwMsrs)
}

/// PROVIDED. Logs the chosen controls and insists on EPT + unrestricted guest, which
/// stages 8-11 need (QEMU: `-cpu host,vmx=on` on a host whose L0 exposes nested EPT).
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let cfg = hw_vmcs_config()?;
    pr_info!(
        "ktvmx: vmcs_config: rev {} size {} pin {:#x} cpu {:#x} cpu2 {:#x} exit {:#x} entry {:#x}\n",
        cfg.revision_id,
        cfg.size,
        cfg.pin_based_exec_ctrl,
        cfg.cpu_based_exec_ctrl,
        cfg.cpu_based_2nd_exec_ctrl,
        cfg.vmexit_ctrl,
        cfg.vmentry_ctrl
    );
    pr_info!(
        "ktvmx: cr0 fixed {:#x}/{:#x} cr4 fixed {:#x}/{:#x} ept_vpid_cap {:#x}\n",
        cfg.cr0_fixed0,
        cfg.cr0_fixed1,
        cfg.cr4_fixed0,
        cfg.cr4_fixed1,
        cfg.ept_vpid_cap
    );
    if cfg.cpu_based_exec_ctrl & CPU_BASED_ACTIVATE_SECONDARY_CONTROLS == 0 {
        return Err("no secondary controls: no EPT, no unrestricted guest");
    }
    let s = cfg.cpu_based_2nd_exec_ctrl;
    if s & SECONDARY_EXEC_ENABLE_EPT == 0 {
        return Err("EPT not available (stages 8+ need it)");
    }
    if s & SECONDARY_EXEC_UNRESTRICTED_GUEST == 0 {
        return Err("unrestricted guest not available (real-mode guests need it)");
    }
    if s & SECONDARY_EXEC_ENABLE_VPID == 0 {
        pr_warn!("ktvmx: no VPID (stage 10 will skip it)\n");
    }
    if cfg.pin_based_exec_ctrl & PIN_BASED_VMX_PREEMPTION_TIMER == 0 {
        pr_warn!("ktvmx: no VMX-preemption timer (stage 9 will skip it)\n");
    }
    Ok(())
}
