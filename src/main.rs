mod kvm;
use std::{env, fs::File, io::Read};

use crate::kvm::VM;
use anyhow::Result;

fn main() -> Result<()> {
    let mut code = Vec::new();
    File::open(env::args().nth(1).expect("no argument passed"))?.read_to_end(&mut code)?;
    println!(
        "Succeffully loading guest code to memory (size: {})",
        code.len()
    );

    let mut vm = VM::new().expect("unable to open VM fd");
    vm.insert_memory_region(0, 1u32, 250 * 1024 * 1024, 0)?;
    vm.create_vcpu()?;
    vm.enter_long_mode()?;
    vm.load_code(&code)?;
    vm.run()?;

    Ok(())
}
