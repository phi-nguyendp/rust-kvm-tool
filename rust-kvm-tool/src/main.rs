mod bootparams;
mod kvm;
mod memory;
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
    vm.setup_irqchip()?;
    vm.create_vcpu()?;
    vm.enter_long_mode()?;
    let entry = vm.load_code(&code)?;
    println!("Code will start at {:#x}", entry);

    // Optional 2nd arg: an initramfs (cpio) image, e.g. built with
    // tools/mkinitramfs.py. If given, it's loaded and the kernel's default
    // `/init` lookup inside it takes over.
    if let Some(initrd_path) = env::args().nth(2) {
        let mut initrd = Vec::new();
        File::open(initrd_path)?.read_to_end(&mut initrd)?;
        let initrd_addr = vm.load_initrd(&initrd)?;
        println!(
            "Loaded initrd (size: {}) at {:#x}",
            initrd.len(),
            initrd_addr
        );
    }

    vm.run(entry)?;

    Ok(())
}
