// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 01: run `vmx_probe()` (src/vmx_caps.rs) against the real CPU.
//! NOT COMPILED by the course author: expect a small fix or two (see CLAUDE.md).

use kernel::prelude::*;

use crate::{hw, vmcs_ops::MsrOps, vmx::*, vmx_caps::*};

/// Real `rdmsr`. A missing MSR would be a #GP (oops), which is why `vmx_probe` must
/// only read MSRs the SDM guarantees to exist. (The kernel itself uses `rdmsrq_safe`.)
pub(crate) struct HwMsrs;

impl MsrOps for HwMsrs {
    fn rdmsr(&self, msr: u32) -> Option<u64> {
        Some(hw::rdmsr(msr))
    }
}

/// PROVIDED. Logs what the CPU supports and checks what the later stages need.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let (_, _, ecx, _) = hw::cpuid(1, 0);
    let caps = vmx_probe(&HwMsrs, ecx).map_err(|e| match e {
        vmx_probe_error::NoVmx => "CPUID.1:ECX.VMX is clear (QEMU needs -cpu host,vmx=on)",
        vmx_probe_error::DisabledByBios => "IA32_FEATURE_CONTROL locks VMX off",
        vmx_probe_error::MsrMissing(_) => "a VMX capability MSR is missing",
    })?;
    pr_info!(
        "ktvmx: vmx_basic: revision {} vmcs_size {} memtype {} true_controls {}\n",
        caps.basic.revision_id,
        caps.basic.vmcs_size,
        caps.basic.memtype,
        caps.basic.true_controls
    );
    if let Some(c) = caps.ept_vpid {
        pr_info!(
            "ktvmx: ept: 4-level {} wb {} 2M {} 1G {} ad {} invept {} | vpid {}\n",
            c.pwl4,
            c.wb,
            c.large_2m,
            c.large_1g,
            c.ad,
            c.invept,
            c.invvpid
        );
    } else {
        pr_info!("ktvmx: no EPT/VPID\n");
    }
    if caps.basic.revision_id == 0 || caps.basic.vmcs_size == 0 {
        return Err("implausible IA32_VMX_BASIC");
    }
    if caps.basic.memtype != 6 {
        return Err("VMCS memory type is not write-back");
    }
    Ok(())
}
