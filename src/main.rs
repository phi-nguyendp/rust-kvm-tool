mod kvm;
use crate::kvm::VM;
use anyhow::Result;

fn main() -> Result<()> {
    let mut vm = VM::new().expect("unable to open VM fd");
    vm.insert_memory_region(0, 1u32, 250 * 1024 * 1024, 0)?;
    vm.create_vcpu()?;
    vm.enter_long_mode()?;

    Ok(())
}
