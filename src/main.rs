mod kvm;
use crate::kvm::VM;

fn main() {
    let vm = VM::new().expect("unable to open VM fd");
    println!("KVM fd: {}", vm.fd);
}
