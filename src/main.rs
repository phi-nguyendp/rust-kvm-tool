mod bootparams;
mod kvm;
use std::{env, fs::File, io::Read};

use crate::kvm::VM;
use anyhow::Result;

fn main() -> Result<()> {
    let mut code = Vec::new();
    File::open(env::args().nth(1).expect("no argument passed"))?.read_to_end(&mut code)?;
    println!(
        "Succeffully loading vmlinux to memory (size: {})",
        code.len()
    );

    let mut vm = VM::new().expect("unable to open VM fd");
    // 2GB ram
    vm.insert_memory_region(0, 1u32, 2 * 1024 * 1024 * 1024, 0)?;
    vm.create_vcpu()?;
    vm.enter_long_mode()?;
    let entry = vm.load_code(&code)?;
    println!("Code will start at {:#x}", entry);
    vm.run(entry)?;

    Ok(())
}
