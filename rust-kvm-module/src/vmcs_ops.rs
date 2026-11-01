//! The hardware seam. PROVIDED: nothing to implement here.
//!
//! The kernel's vmx.c calls `vmcs_readl()/vmcs_writel()` (arch/x86/kvm/vmx/vmx_ops.h)
//! and `rdmsr()` directly. We put a trait in front so that the same stage logic runs
//! (a) on the host in `cargo test`, against `mock::MockVmcs`/`mock::MockMsrs`, and
//! (b) in the kernel module, against real `vmread`/`vmwrite`/`rdmsr` (module/hw.rs).

/// Access to the *current* VMCS (the one made current by VMPTRLD).
pub trait VmcsOps {
    fn vmcs_readl(&self, field: u32) -> u64;
    fn vmcs_writel(&mut self, field: u32, value: u64);

    fn vmcs_read16(&self, field: u32) -> u16 {
        self.vmcs_readl(field) as u16
    }
    fn vmcs_read32(&self, field: u32) -> u32 {
        self.vmcs_readl(field) as u32
    }
    fn vmcs_read64(&self, field: u32) -> u64 {
        self.vmcs_readl(field)
    }
    fn vmcs_write16(&mut self, field: u32, value: u16) {
        self.vmcs_writel(field, value as u64)
    }
    fn vmcs_write32(&mut self, field: u32, value: u32) {
        self.vmcs_writel(field, value as u64)
    }
    fn vmcs_write64(&mut self, field: u32, value: u64) {
        self.vmcs_writel(field, value)
    }
    fn vmcs_set_bits(&mut self, field: u32, mask: u64) {
        let v = self.vmcs_readl(field);
        self.vmcs_writel(field, v | mask)
    }
    fn vmcs_clear_bits(&mut self, field: u32, mask: u64) {
        let v = self.vmcs_readl(field);
        self.vmcs_writel(field, v & !mask)
    }
}

/// Read access to model-specific registers (`rdmsr`).
pub trait MsrOps {
    /// Returns `None` if the MSR does not exist (the real `rdmsr` would #GP).
    fn rdmsr(&self, msr: u32) -> Option<u64>;
}

/// Read access to guest-physical memory as the VMM sees it (module-allocated RAM).
pub trait GuestMem {
    fn read(&self, gpa: u64, buf: &mut [u8]) -> bool;
    fn write(&mut self, gpa: u64, buf: &[u8]) -> bool;
}
