//! Host-only test doubles for the hardware seam. PROVIDED.
use crate::vmcs_ops::*;
use std::collections::HashMap;

/// A fake "current VMCS": every field reads 0 until written, like a freshly
/// VMCLEARed VMCS. Also records the write order so tests can assert on it.
#[derive(Default, Clone, Debug)]
pub struct MockVmcs {
    pub fields: HashMap<u32, u64>,
    pub writes: Vec<(u32, u64)>,
}

impl VmcsOps for MockVmcs {
    fn vmcs_readl(&self, field: u32) -> u64 {
        self.fields.get(&field).copied().unwrap_or(0)
    }
    fn vmcs_writel(&mut self, field: u32, value: u64) {
        self.fields.insert(field, value);
        self.writes.push((field, value));
    }
}

/// A fake MSR file.
#[derive(Default, Clone, Debug)]
pub struct MockMsrs(pub HashMap<u32, u64>);

impl MockMsrs {
    pub fn with(mut self, msr: u32, val: u64) -> Self {
        self.0.insert(msr, val);
        self
    }
}

impl MsrOps for MockMsrs {
    fn rdmsr(&self, msr: u32) -> Option<u64> {
        self.0.get(&msr).copied()
    }
}

/// Flat guest RAM starting at GPA 0.
pub struct MockGuestMem(pub Vec<u8>);

impl MockGuestMem {
    pub fn new(size: usize) -> Self {
        Self(vec![0; size])
    }
}

impl GuestMem for MockGuestMem {
    fn read(&self, gpa: u64, buf: &mut [u8]) -> bool {
        let a = gpa as usize;
        match self.0.get(a..a + buf.len()) {
            Some(s) => {
                buf.copy_from_slice(s);
                true
            }
            None => false,
        }
    }
    fn write(&mut self, gpa: u64, buf: &[u8]) -> bool {
        let a = gpa as usize;
        match self.0.get_mut(a..a + buf.len()) {
            Some(s) => {
                s.copy_from_slice(buf);
                true
            }
            None => false,
        }
    }
}
