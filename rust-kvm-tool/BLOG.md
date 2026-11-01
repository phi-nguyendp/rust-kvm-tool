# Building a Minimal Linux VMM in Rust: Booting vmlinux with KVM

Have you ever wondered how virtual machines actually work under the hood? In this article, I'll walk you through my journey of building a minimal Virtual Machine Monitor (VMM) in Rust that can boot a Linux kernel directly using KVM (Kernel-based Virtual Machine).

But before we dive into code, let's establish a solid foundation by understanding the key concepts and terminology in virtualization.

---

## Understanding Virtualization: The Foundation

### What is Virtualization?

**Virtualization** is a technology that allows you to create multiple simulated computing environments (called "virtual machines") from a single physical hardware system. Think of it as creating multiple "imaginary computers" that all share the same physical CPU, memory, and storage.

```
Physical Reality:                    What Software Sees:
┌─────────────────────┐             ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│   Physical Machine  │             │    VM 1     │ │    VM 2     │ │    VM 3     │
│   - 1 CPU (8 cores) │  ────────►  │  "2 CPUs"   │ │  "2 CPUs"   │ │  "4 CPUs"   │
│   - 64 GB RAM       │  Virtualize │  "16GB RAM" │ │  "16GB RAM" │ │  "32GB RAM" │
│   - 1 TB Storage    │             │  "200GB"    │ │  "300GB"    │ │  "500GB"    │
└─────────────────────┘             └─────────────┘ └─────────────┘ └─────────────┘
```

Each virtual machine believes it has its own dedicated hardware, but in reality, they're all sharing the same physical resources, managed carefully by a special layer of software.

### Why Virtualization Matters

Virtualization solves several important problems:

1. **Resource Efficiency**: Instead of one server running one OS at 10% CPU usage, run 10 VMs on one server at 100% utilization.

2. **Isolation**: If one VM crashes or gets compromised, others are unaffected. Each VM is a sandbox.

3. **Flexibility**: Spin up new "machines" in seconds without buying hardware. Test different OS versions side by side.

4. **Legacy Support**: Run old operating systems that require specific hardware on modern machines.

### What is a Virtual Machine (VM)?

A **Virtual Machine** is a software-based emulation of a physical computer. From the perspective of software running inside it:

- It appears to be a complete, standalone computer
- It has its own CPU, memory, storage, and network interfaces
- It can run its own operating system (called the **guest OS**)
- It is completely unaware that it's not running on real hardware

```
┌─────────────────────────────────────────────────────┐
│                  Virtual Machine                     │
│  ┌───────────────────────────────────────────────┐  │
│  │              Guest Operating System            │  │
│  │         (Linux, Windows, FreeBSD, etc.)        │  │
│  ├───────────────────────────────────────────────┤  │
│  │              Guest Applications                │  │
│  │         (Web servers, databases, etc.)         │  │
│  └───────────────────────────────────────────────┘  │
│                         │                            │
│                         ▼                            │
│  ┌───────────────────────────────────────────────┐  │
│  │           Virtual Hardware                     │  │
│  │  vCPU    vRAM    vDisk    vNIC    vGPU        │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
                          │
            The guest thinks this is real hardware,
            but it's all simulated by the VMM/Hypervisor
```

The "v" prefix (vCPU, vRAM, etc.) stands for "virtual" - these are software abstractions that look like real hardware to the guest.

### What is a Virtual Machine Monitor (VMM) / Hypervisor?

A **Virtual Machine Monitor (VMM)**, also called a **Hypervisor**, is the software layer that creates and manages virtual machines. It's the "magic" that makes virtualization possible.

The terms VMM and Hypervisor are often used interchangeably, though historically:
- **VMM** emphasizes the monitoring/management aspect
- **Hypervisor** emphasizes that it runs "above" (hyper) the supervisor (kernel)

The VMM/Hypervisor has several critical responsibilities:

#### 1. Resource Multiplexing
The VMM shares physical resources among multiple VMs:
```
                    VMM/Hypervisor
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
   ┌─────────┐      ┌─────────┐      ┌─────────┐
   │  VM 1   │      │  VM 2   │      │  VM 3   │
   │ 2 vCPUs │      │ 4 vCPUs │      │ 2 vCPUs │
   └─────────┘      └─────────┘      └─────────┘
        │                │                │
        └────────────────┴────────────────┘
                         │
                         ▼
              Physical CPU (8 cores)
              VMM schedules which vCPU
              runs on which physical core
```

#### 2. Isolation Enforcement
The VMM ensures VMs cannot interfere with each other:
- VM 1 cannot read VM 2's memory
- VM 1 cannot access VM 2's disk
- A crash in VM 1 doesn't affect VM 2

#### 3. Hardware Emulation / Virtualization
The VMM presents virtual hardware to each VM:
- Virtual disk (actually a file on the host)
- Virtual network card (traffic routed by VMM)
- Virtual serial ports, USB, etc.

#### 4. Privilege Management
The VMM intercepts sensitive operations from the guest:
- When the guest tries to access hardware → VMM handles it
- When the guest modifies page tables → VMM validates it
- When the guest executes privileged instructions → VMM emulates or permits

### Types of Hypervisors: Type 1 vs Type 2

Hypervisors are classified into two types based on where they run:

#### Type 1: Bare-Metal Hypervisor

A Type 1 hypervisor runs **directly on the hardware**, without a host operating system beneath it. It IS the operating system (or is tightly integrated with one).

```
┌─────────┐ ┌─────────┐ ┌─────────┐
│  VM 1   │ │  VM 2   │ │  VM 3   │
│ (Guest) │ │ (Guest) │ │ (Guest) │
└────┬────┘ └────┬────┘ └────┬────┘
     │           │           │
     └───────────┴───────────┘
                 │
     ┌───────────▼───────────┐
     │   Type 1 Hypervisor   │
     │ (runs on bare metal)  │
     └───────────┬───────────┘
                 │
     ┌───────────▼───────────┐
     │       Hardware        │
     └───────────────────────┘
```

**Examples**: VMware ESXi, Microsoft Hyper-V, Xen, KVM (with Linux as the host)

**Advantages**:
- Better performance (no host OS overhead)
- More secure (smaller attack surface)
- Direct hardware access

#### Type 2: Hosted Hypervisor

A Type 2 hypervisor runs **on top of a host operating system**, like a regular application.

```
┌─────────┐ ┌─────────┐
│  VM 1   │ │  VM 2   │
│ (Guest) │ │ (Guest) │
└────┬────┘ └────┬────┘
     │           │
     └─────┬─────┘
           │
┌──────────▼──────────┐
│  Type 2 Hypervisor  │  ◄── Runs as an application
│  (VirtualBox, etc.) │
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│   Host OS (Windows, │  ◄── Regular operating system
│   macOS, Linux)     │
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│      Hardware       │
└─────────────────────┘
```

**Examples**: VirtualBox, VMware Workstation, Parallels Desktop, QEMU (without KVM)

**Advantages**:
- Easy to install (just another app)
- Good for development/testing
- Runs alongside other applications

**Disadvantages**:
- More overhead (host OS + hypervisor)
- Less direct hardware access

### Where Does KVM Fit?

KVM (Kernel-based Virtual Machine) is interesting because it blurs the line between Type 1 and Type 2:

```
┌─────────┐ ┌─────────┐   ┌────────────────┐
│  VM 1   │ │  VM 2   │   │ Regular Apps   │
│ (Guest) │ │ (Guest) │   │ (Firefox, etc) │
└────┬────┘ └────┬────┘   └───────┬────────┘
     │           │                │
     └─────┬─────┘                │
           │                      │
┌──────────▼──────────────────────▼────────┐
│                Linux Kernel               │
│  ┌─────────────┐                         │
│  │ KVM Module  │  ◄── Turns Linux into   │
│  │             │      a hypervisor       │
│  └─────────────┘                         │
└──────────────────────┬───────────────────┘
                       │
┌──────────────────────▼───────────────────┐
│   Hardware (Intel VT-x / AMD-V)          │
└──────────────────────────────────────────┘
```

**KVM is technically a Type 1 hypervisor** because:
- It's a kernel module that runs at the highest privilege level
- It directly uses hardware virtualization extensions
- The Linux kernel becomes the hypervisor

But it **feels like Type 2** because:
- You can run regular applications alongside VMs
- It integrates with existing Linux systems
- VMMs like QEMU run as userspace processes

This hybrid approach gives the best of both worlds: near-native performance with the convenience of a regular Linux system.

### The Evolution of Virtualization

Understanding history helps explain why things work the way they do:

#### Era 1: Software-Only Virtualization (1990s-2005)

Without hardware support, VMMs had to use tricks:

**Binary Translation**: Scan guest code, find privileged instructions, replace them with safe equivalents.
```
Guest code:                    Translated code:
mov cr3, eax    ──────────►    call vmm_handle_cr3_write
                               (VMM intercepts and emulates)
```

**Paravirtualization**: Modify the guest OS to call the VMM directly instead of executing privileged instructions.

These worked but were slow (10-40% overhead) and complex.

#### Era 2: Hardware-Assisted Virtualization (2005-Present)

Intel (VT-x, 2005) and AMD (AMD-V, 2006) added CPU extensions for virtualization:

- **New CPU mode**: "Guest mode" / "Non-root mode"
- **Automatic trapping**: CPU traps to VMM on sensitive operations
- **Hardware memory translation**: Nested/Extended Page Tables (EPT/NPT)

```
Without Hardware Support:              With Hardware Support (VT-x/AMD-V):
┌─────────────────────────┐           ┌─────────────────────────┐
│ Guest executes          │           │ Guest executes          │
│ privileged instruction  │           │ privileged instruction  │
└───────────┬─────────────┘           └───────────┬─────────────┘
            │                                     │
            ▼                                     ▼
┌─────────────────────────┐           ┌─────────────────────────┐
│ VMM must detect via     │           │ CPU automatically traps │
│ binary translation      │           │ to VMM (hardware does   │
│ (scan every instruction)│           │ the detection)          │
└───────────┬─────────────┘           └───────────┬─────────────┘
            │                                     │
            ▼                                     ▼
     Slow, complex                         Fast, simple
     10-40% overhead                       <5% overhead
```

**KVM was designed specifically to use these hardware extensions**, which is why it's so simple yet so fast.

### Key Terms Summary

| Term | Definition |
|------|------------|
| **Virtualization** | Technology to create multiple simulated computers from one physical machine |
| **Virtual Machine (VM)** | A software emulation of a computer that can run its own OS |
| **Guest OS** | The operating system running inside a VM |
| **Host OS** | The operating system running on the physical machine |
| **VMM / Hypervisor** | Software that creates and manages VMs |
| **Type 1 Hypervisor** | Runs directly on hardware (bare-metal) |
| **Type 2 Hypervisor** | Runs on top of a host OS |
| **vCPU** | Virtual CPU - a software abstraction of a physical CPU core |
| **KVM** | Linux kernel module that turns Linux into a Type 1 hypervisor |
| **VT-x / AMD-V** | Hardware virtualization extensions in Intel/AMD CPUs |
| **VM Exit** | When the CPU transfers control from guest to VMM |
| **VM Entry** | When the CPU transfers control from VMM to guest |

---

## Why Build a VMM from Scratch?

Before diving into the code, let me explain why I embarked on this project:

1. **Deep Understanding**: Production VMMs like QEMU have millions of lines of code. By building a minimal one, I can understand the essential components without getting lost in complexity.

2. **Demystifying Virtualization**: Virtualization often feels like magic. Writing a VMM reveals it's just careful CPU configuration and memory mapping.

3. **Learning x86 Architecture**: There's no better way to understand x86's segmentation, paging, and privilege levels than implementing them yourself.

4. **Rust for Systems Programming**: Rust's safety guarantees are valuable even when doing unsafe low-level work - the compiler catches many bugs at compile time.

## What is KVM and Why Use It?

KVM (Kernel-based Virtual Machine) is a virtualization infrastructure built into the Linux kernel since 2007. But what does that actually mean?

### The Problem KVM Solves

Without hardware virtualization, running a guest OS requires **binary translation** - the VMM must inspect every guest instruction and translate privileged operations into safe equivalents. This is slow and complex.

Modern CPUs (Intel VT-x, AMD-V) solve this with **hardware virtualization extensions**:
- The CPU has a new privilege level called "guest mode"
- Guest code runs directly on the CPU at near-native speed
- When the guest tries to do something privileged (I/O, change page tables), the CPU automatically traps back to the host

### What KVM Provides

KVM exposes this hardware capability through a simple interface:
- `/dev/kvm` - A character device for system-wide operations
- VM file descriptors - Represent individual virtual machines
- vCPU file descriptors - Represent virtual CPUs within a VM

All communication happens through `ioctl()` system calls. This is the interface my VMM uses.

## Project Overview

My project, `rust-kvm-tool`, is a minimal VMM written in Rust that can:

- Create and configure a virtual machine using KVM
- Set up guest memory (2GB RAM)
- Configure a virtual CPU (vCPU) in 64-bit long mode
- Load and execute an ELF vmlinux kernel image
- Emulate a serial console for kernel output

The entire implementation is around 800 lines of Rust code, demonstrating that you don't need tens of thousands of lines to boot a Linux kernel.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Host Userspace                          │
│  ┌───────────────────────────────────────────────────────┐  │
│  │                  rust-kvm-tool                         │  │
│  │  ┌─────────┐  ┌─────────┐  ┌──────────────────────┐   │  │
│  │  │   VM    │  │  vCPU   │  │   Guest Memory       │   │  │
│  │  │ Manager │  │ Manager │  │   (mmap'd region)    │   │  │
│  │  └────┬────┘  └────┬────┘  └──────────┬───────────┘   │  │
│  └───────┼────────────┼─────────────────┼────────────────┘  │
│          │            │                  │                   │
│          │    ioctl() calls              │                   │
│          ▼            ▼                  ▼                   │
├─────────────────────────────────────────────────────────────┤
│                       /dev/kvm                               │
├─────────────────────────────────────────────────────────────┤
│                      Linux Kernel                            │
│                    ┌───────────┐                             │
│                    │    KVM    │                             │
│                    │  Module   │                             │
│                    └─────┬─────┘                             │
├──────────────────────────┼──────────────────────────────────┤
│                    Hardware (CPU)                            │
│              Intel VT-x / AMD-V Extensions                   │
└─────────────────────────────────────────────────────────────┘
```

## Key Components (Deep Dive)

### 1. Opening KVM and Creating a VM

#### Why This Step?

Before we can do anything, we need to establish communication with KVM. The kernel exposes KVM through the `/dev/kvm` character device. Opening this file gives us a file descriptor we can use to issue commands to the KVM subsystem.

#### The Code

```rust
impl Kvm {
    fn new() -> Result<Self> {
        let kvm_path = CString::new("/dev/kvm")?;
        let kvm_fd = unsafe { libc::open(kvm_path.as_ptr(), O_RDWR) };
        // ... error handling
        Ok(Self { fd: unsafe { OwnedFd::from_raw_fd(kvm_fd) } })
    }

    fn setup(&self) -> Result<()> {
        let version = unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_API_VERSION, 0) };
        if version != 12 {
            anyhow::bail!("Unsupported KVM API version: {}", version);
        }
        Ok(())
    }
}

impl VM {
    pub fn new() -> Result<Self> {
        let kvm = Kvm::new()?;
        kvm.setup()?;
        let fd = unsafe { ioctl(kvm.fd.as_raw_fd(), KVM_CREATE_VM, 0) };
        // ... create VM instance
    }
}
```

#### What This Does

1. **`open("/dev/kvm", O_RDWR)`**: Opens the KVM device file with read/write access. This returns a file descriptor that represents our connection to KVM. Without this, we cannot communicate with the virtualization subsystem.

2. **`KVM_GET_API_VERSION`**: Queries the KVM API version. KVM guarantees API stability - version 12 has been stable since 2007. This check ensures our VMM is compatible with the host's KVM.

3. **`KVM_CREATE_VM`**: Creates a new virtual machine and returns a file descriptor representing it. This VM fd is a container - it holds memory regions, vCPUs, and device state. At this point, the VM exists but has no memory, no CPU, and cannot run anything.

#### Effect

After this code runs, we have:
- A connection to KVM (`kvm_fd`)
- An empty VM container (`vm_fd`)
- The ability to configure this VM through subsequent ioctl calls

---

### 2. Guest Memory Management

#### Why This Step?

A virtual machine needs RAM. But unlike a physical machine where RAM is hardware, our VM's "RAM" is just a region of the host's memory that we've designated for guest use.

The key insight is: **guest physical addresses are not host physical addresses**. When the guest accesses address `0x1000`, it shouldn't touch the host's memory at that address. Instead, KVM translates guest physical addresses to host virtual addresses using a mapping we provide.

#### The Code

```rust
pub struct MemoryWrapper {
    ptr: NonNull<c_void>,  // Host virtual address of the allocated memory
    size: usize,           // Size in bytes
}

impl MemoryWrapper {
    pub fn new(mem_size: u64) -> Result<Self> {
        let addr = unsafe {
            mmap(
                core::ptr::null_mut(),   // Let kernel choose address
                mem_size as usize,       // 2GB in our case
                PROT_READ | PROT_WRITE,  // Read and write access
                MAP_ANON | MAP_SHARED,   // Anonymous (no file), shared
                -1,                       // No file descriptor
                0,                        // No offset
            )
        };
        // ... error handling and wrap in NonNull
    }
}
```

Then we register this memory with KVM:

```rust
pub fn insert_memory_region(&mut self, slot: u32, flags: u32, 
                            mem_size: u64, guest_pa: u64) -> Result<()> {
    let host_memory = MemoryWrapper::new(mem_size)?;
    let mem_region = kvm_userspace_memory_region {
        slot,                           // Memory slot index
        flags,                          // KVM_MEM_LOG_DIRTY_PAGES, etc.
        userspace_addr: host_memory,    // Host virtual address
        memory_size: mem_size,          // Size of the region
        guest_phys_addr: guest_pa,      // Where guest sees this memory
    };

    let ret = unsafe {
        ioctl(self.fd.as_raw_fd(), KVM_SET_USER_MEMORY_REGION, &mem_region, 0)
    };
    // ...
}
```

#### What This Does

1. **`mmap()` with `MAP_ANON | MAP_SHARED`**: Allocates a contiguous block of virtual memory from the host. `MAP_ANON` means it's not backed by any file (just zeroed memory). `MAP_SHARED` is required because KVM will access this memory from kernel context.

2. **`KVM_SET_USER_MEMORY_REGION`**: This is the critical step. It tells KVM: "When the guest accesses physical address `guest_phys_addr`, redirect that access to host virtual address `userspace_addr`". 

#### Effect

After this code:
- The guest has 2GB of "RAM" starting at physical address 0
- When guest code reads/writes memory, KVM translates the address
- We can write to `host_memory` from the host side to load code/data
- The memory is initialized to zeros

#### Memory Translation Example

```
Guest perspective:          Host perspective:
┌────────────────┐          ┌────────────────────────┐
│ Physical: 0x0  │ ──KVM──► │ Virtual: 0x7f4a00000000│
│ (Guest RAM)    │ translates│ (mmap'd region)        │
└────────────────┘          └────────────────────────┘
```

---

### 3. Setting Up 64-bit Long Mode

#### Why This Step?

When a x86 CPU powers on, it starts in **real mode** - a 16-bit mode dating back to the 8086. To run a modern 64-bit Linux kernel, we must configure the CPU to enter **long mode** (64-bit mode).

This isn't a simple switch - it requires setting up:
1. **Page tables**: Long mode requires paging
2. **Segment descriptors**: Even in 64-bit mode, we need valid segments
3. **Control registers**: Multiple bits must be set correctly

Getting any of these wrong causes a VM entry failure.

#### 3a. Page Tables (Identity Mapping)

##### Why Identity Mapping?

When Linux boots, it expects **identity mapping**: virtual address X maps to physical address X. This is necessary because:
- The kernel's early boot code has hardcoded physical addresses
- Before Linux sets up its own page tables, it needs to execute from known locations
- The transition from real mode to protected mode to long mode needs consistent addressing

##### The Code

```rust
fn enter_long_mode(&self, mem: &mut [u8]) -> Result<()> {
    // Page table locations in guest physical memory
    let pml4_addr: u64 = 0x2000;  // Page Map Level 4
    let pdpt_addr: u64 = 0x3000;  // Page Directory Pointer Table
    let pd_addr: u64 = 0x4000;    // Page Directory

    // Page table entry flags
    let flags: u64 = (1 << 0)     // Present - page is valid
                   | (1 << 1);    // Read/Write - page is writable

    // PML4[0] points to PDPT
    // This single entry covers the first 512GB of address space
    let entry = pdpt_addr | flags;
    mem[0x2000..0x2008].copy_from_slice(&entry.to_le_bytes());
    
    // PDPT[0] points to PD
    // This single entry covers the first 1GB of address space
    let entry = pd_addr | flags;
    mem[0x3000..0x3008].copy_from_slice(&entry.to_le_bytes());
    
    // PD entries: each covers 2MB (using huge pages)
    // 512 entries × 2MB = 1GB of identity-mapped memory
    let huge_page_flags = flags | 0x80;  // PS bit = Page Size (2MB pages)
    for i in 0..512 {
        let phys_addr = i * 0x200000;    // 2MB aligned physical address
        let entry = phys_addr | huge_page_flags;
        let offset = 0x4000 + (i as usize * 8);
        mem[offset..offset + 8].copy_from_slice(&entry.to_le_bytes());
    }
    // ...
}
```

##### What This Does

This creates a 4-level page table structure (required for 64-bit mode):

```
Virtual Address Translation (4-level paging):

 63    48 47    39 38    30 29    21 20       0
┌────────┬────────┬────────┬────────┬──────────┐
│ Sign   │ PML4   │ PDPT   │  PD    │  Offset  │
│ Extend │ Index  │ Index  │ Index  │ (2MB)    │
└────────┴───┬────┴───┬────┴───┬────┴────┬─────┘
             │        │        │         │
             ▼        ▼        ▼         │
          ┌─────┐ ┌─────┐ ┌─────┐       │
          │PML4 │→│PDPT │→│ PD  │       │
          │@2000│ │@3000│ │@4000│       │
          └─────┘ └─────┘ └──┬──┘       │
                             │          │
                    2MB Page ▼          ▼
                    ┌──────────────────────┐
                    │   Physical Memory    │
                    │   (identity mapped)  │
                    └──────────────────────┘
```

Each page table entry is 8 bytes, containing:
- **Bits 0**: Present - if 0, accessing this page causes a fault
- **Bit 1**: Read/Write - if 0, writes cause a fault
- **Bit 7 (PS)**: Page Size - if 1 in PD, this is a 2MB huge page
- **Bits 12-51**: Physical address of next level (or final page)

##### Effect

After writing these page tables:
- Virtual address `0x0` maps to physical address `0x0`
- Virtual address `0x1000` maps to physical address `0x1000`
- This continues up to 1GB (512 × 2MB pages)
- The kernel can execute without worrying about address translation

#### 3b. Segment Descriptors and GDT

##### Why We Need Segments in 64-bit Mode

You might think segments are obsolete in 64-bit mode - and you'd be partially right. Long mode largely ignores segment bases and limits for CS/DS/ES/SS. However:

1. The CPU still checks that segments are **valid** and **present**
2. The **CS** segment determines if code runs in 64-bit or 32-bit compatibility mode
3. The **L bit** (Long mode) in CS must be set for 64-bit execution
4. Without valid segments, the CPU refuses to enter guest mode

##### The Code

```rust
const fn seg_with_st(selector_index: u16, type_: u8) -> kvm_segment {
    kvm_segment {
        base: 0,              // Base address (ignored in long mode for most segments)
        limit: 0xffffffff,    // Segment limit (ignored in long mode)
        selector: selector_index << 3,  // Selector = index × 8 (each GDT entry is 8 bytes)
        type_,                // Segment type (code/data, permissions)
        present: 1,           // Segment is valid
        dpl: 0,               // Privilege level 0 (kernel)
        db: if type_ == 0b1011 { 0 } else { 1 },  // D/B flag
        s: 1,                 // Not a system segment
        l: if type_ == 0b1011 { 1 } else { 0 },   // Long mode bit (code only)
        g: 1,                 // Granularity (limit × 4KB)
        avl: 0,               // Available for OS use
        padding: 0,
    }
}

// Code segment: selector index 1, type 0b1011 (Execute/Read, accessed)
const CODE_SEG: kvm_segment = seg_with_st(1, 0b1011);

// Data segment: selector index 2, type 0b0011 (Read/Write, accessed)  
const DATA_SEG: kvm_segment = seg_with_st(2, 0b0011);
```

##### What the Type Field Means

The segment type is a 4-bit field:

```
For Code Segments (bit 3 = 1):
  Bit 3: 1 = Code segment
  Bit 2: Conforming (0 = non-conforming)
  Bit 1: Readable (1 = can read code)
  Bit 0: Accessed (set by CPU on access)
  
  0b1011 = Code, Non-conforming, Readable, Accessed

For Data Segments (bit 3 = 0):
  Bit 3: 0 = Data segment
  Bit 2: Expand-down (0 = expand-up)
  Bit 1: Writable (1 = can write)
  Bit 0: Accessed
  
  0b0011 = Data, Expand-up, Writable, Accessed
```

##### The Critical L Bit

```rust
l: if type_ == 0b1011 { 1 } else { 0 },  // Long mode bit
db: if type_ == 0b1011 { 0 } else { 1 }, // D/B must be 0 when L=1
```

For 64-bit code:
- **L = 1**: This is a 64-bit code segment
- **D = 0**: Required when L=1 (Intel specification)

If you set both L=1 and D=1, the CPU generates a fault.

##### Writing the GDT to Memory

```rust
let gdt_table: [u64; 3] = [
    0,                       // Entry 0: NULL descriptor (required)
    to_gdt_entry(&CODE_SEG), // Entry 1: Code segment
    to_gdt_entry(&DATA_SEG), // Entry 2: Data segment
];

// Copy GDT to guest memory at address 0x500
mem[0x500..0x500 + gdt_bytes.len()].copy_from_slice(gdt_bytes);

// Tell the CPU where the GDT is
sregs.gdt.base = 0x500;
sregs.gdt.limit = std::mem::size_of_val(&gdt_table) as u16 - 1;
```

##### Effect

The GDT in guest memory looks like:

```
Address 0x500:
┌────────────────────────┐
│ 0x0000000000000000     │ NULL descriptor (index 0)
├────────────────────────┤
│ 0x00AF9A000000FFFF     │ Code segment (index 1, selector 0x08)
├────────────────────────┤
│ 0x00CF92000000FFFF     │ Data segment (index 2, selector 0x10)
└────────────────────────┘
```

#### 3c. Control Registers

##### Why Each Register Matters

```rust
// Get current special registers
let mut sregs = kvm_sregs::default();
unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_SREGS, &mut sregs, 0) };

// CR3: Page table base address
sregs.cr3 = pml4_addr;  // 0x2000 - tells CPU where page tables are

// CR0: System control flags
sregs.cr0 |= X86_CR0_PE;  // 0x1 - Protection Enable (enter protected mode)
sregs.cr0 |= X86_CR0_PG;  // 0x80000000 - Paging Enable

// CR4: Extended features
sregs.cr4 |= X86_CR4_PAE;  // 0x20 - Physical Address Extension (required for long mode)
sregs.cr4 |= X86_CR4_PSE;  // 0x10 - Page Size Extension (allow 2MB/4MB pages)

// EFER: Extended Feature Enable Register (MSR)
sregs.efer |= EFER_LME;   // 0x100 - Long Mode Enable
sregs.efer |= EFER_LMA;   // 0x400 - Long Mode Active

unsafe { ioctl(self.fd.as_raw_fd(), KVM_SET_SREGS, &sregs, 0) };
```

##### What Each Bit Does

**CR0 (Control Register 0)**:
- `PE` (bit 0): Switches from real mode to protected mode. Without this, segmentation doesn't work.
- `PG` (bit 31): Enables paging. Virtual addresses start being translated through page tables.

**CR3 (Control Register 3)**:
- Contains the physical address of the PML4 table
- When the CPU needs to translate an address, it starts here

**CR4 (Control Register 4)**:
- `PAE` (bit 5): Enables Physical Address Extension. Required for long mode because 64-bit paging uses the PAE format.
- `PSE` (bit 4): Allows 2MB and 4MB huge pages.

**EFER (Extended Feature Enable Register)**:
- `LME` (bit 8): Long Mode Enable - tells the CPU we want 64-bit mode
- `LMA` (bit 10): Long Mode Active - set by CPU when long mode is actually active

##### Effect

After setting these registers:
- The CPU is in 64-bit long mode
- Paging is enabled with our identity-mapped page tables
- All segment registers point to valid descriptors
- Guest code will execute as 64-bit instructions

---

### 4. Linux Boot Protocol

#### Why Boot Protocol Matters

You can't just jump to the kernel's entry point and hope for the best. Linux expects a specific environment when it starts:

1. **`boot_params` structure**: Contains hardware information, memory map, command line
2. **Specific register state**: RSI must point to boot_params
3. **Memory layout**: Certain addresses have specific purposes

Without this, the kernel crashes immediately because it can't find basic information.

#### The Code

```rust
fn setup_boot_params(boot_params: &mut boot_params, cmdline_addr: u32, total_mem: u64) {
    // 1. Bootloader identification
    // 0xFF = "Unregistered bootloader" - Linux accepts any value here
    boot_params.hdr.type_of_loader = 0xFF;
    
    // Magic numbers Linux checks to verify boot_params is valid
    boot_params.hdr.boot_flag = 0xAA55;        // Same as MBR signature
    boot_params.hdr.header = 0x53726448;       // "HdrS" in ASCII
    
    // 2. Command line setup
    // This is how we pass "console=ttyS0" to the kernel
    boot_params.hdr.cmd_line_ptr = cmdline_addr;  // 0x20000 in our case
    boot_params.hdr.cmdline_size = CMDLINE.len() as u32;
    
    // 3. Heap configuration
    // The kernel uses this for early allocations before proper memory management
    boot_params.hdr.loadflags = CAN_USE_HEAP | 0x01 | KEEP_SEGMENTS;
    boot_params.hdr.heap_end_ptr = 0xFE00;
    
    // 4. E820 Memory Map - THIS IS CRITICAL
    // Linux uses E820 to know what memory is available
    boot_params.e820_entries = 2;
    
    // Low memory: 0 to 640KB (conventional memory)
    // The "hole" from 640KB to 1MB is reserved for legacy hardware
    boot_params.e820_table[0].addr = 0x0;
    boot_params.e820_table[0].size = 0x9FC00;  // 639KB
    boot_params.e820_table[0].type_ = E820_RAM;
    
    // High memory: 1MB to end of RAM
    // This is where the kernel and most data lives
    boot_params.e820_table[1].addr = 0x100000;  // 1MB
    boot_params.e820_table[1].size = total_mem - 0x100000;
    boot_params.e820_table[1].type_ = E820_RAM;
}
```

#### What the E820 Memory Map Does

The E820 memory map is how BIOS/UEFI tells the kernel about system memory. Each entry has:
- **addr**: Start of the region
- **size**: Size in bytes
- **type**: What kind of region (RAM, reserved, ACPI, etc.)

Why two regions with a gap?

```
0x00000 ┌─────────────────┐
        │   Low Memory    │ ◄── Usable RAM (640KB)
        │   (E820_RAM)    │
0x9FC00 ├─────────────────┤
        │   Legacy Hole   │ ◄── Reserved for VGA, ROMs, etc.
        │   (not mapped)  │
0x100000├─────────────────┤
        │   High Memory   │ ◄── Usable RAM (rest of memory)
        │   (E820_RAM)    │
        │                 │
        └─────────────────┘
```

This layout mimics real PC hardware for compatibility.

#### Placing boot_params in Memory

```rust
pub fn load_code(&self, vmlinux: &Vec<u8>) -> Result<__u64> {
    // ... load kernel ...
    
    // Copy boot_params to 0x10000 (64KB)
    unsafe {
        std::ptr::copy_nonoverlapping(
            &boot_params as *const boot_params as *const u8,
            self.memory_regions.userspace_addr.as_mut_ptr().add(0x10000),
            size_of_val(&boot_params),
        );
        
        // Copy command line to 0x20000 (128KB)
        std::ptr::copy_nonoverlapping(
            CMDLINE.as_ptr(),  // "console=ttyS0 earlyprintk=ttyS0"
            self.memory_regions.userspace_addr.as_mut_ptr().add(0x20000),
            CMDLINE.len(),
        );
    }
}
```

#### Effect

After setup:
- `boot_params` is at guest physical address 0x10000
- Command line "console=ttyS0 earlyprintk=ttyS0" is at 0x20000
- The kernel can find memory information, boot options, and hardware details
- Without this, you'd see: `Kernel panic - not syncing: No E820 memory map`

---

### 5. Loading the vmlinux ELF

#### Why ELF Parsing?

vmlinux is the uncompressed Linux kernel in ELF format. ELF (Executable and Linkable Format) is a standard format that specifies:
- Where in memory each section should be loaded
- What permissions each section needs (read, write, execute)
- The entry point address

We can't just copy the file to memory - we need to parse it and load each segment to its correct address.

#### The Code

```rust
pub fn load_code(&self, vmlinux: &Vec<u8>) -> Result<__u64> {
    // Parse ELF headers
    let elf = ElfFile::new(vmlinux).map_err(|e| anyhow::anyhow!(e))?;
    
    // Iterate through program headers (segments)
    for ph in elf.program_iter() {
        // Only load PT_LOAD segments (actual code/data)
        if ph.get_type().unwrap_or(xmas_elf::program::Type::Null)
            == xmas_elf::program::Type::Load
        {
            let offset = ph.offset() as usize;      // Offset in ELF file
            let file_size = ph.file_size() as usize; // Bytes in file
            let mem_size = ph.mem_size() as usize;   // Bytes in memory
            let paddr = ph.physical_addr() as usize; // Guest physical address

            unsafe {
                let dest = self.memory_regions.userspace_addr.as_mut_ptr();
                let src = vmlinux[offset..(offset + file_size)].as_ptr();

                // Copy segment from ELF to guest memory
                std::ptr::copy_nonoverlapping(src, dest.add(paddr), file_size);

                // Zero BSS (mem_size > file_size means uninitialized data)
                if mem_size > file_size {
                    std::ptr::write_bytes(
                        dest.add(paddr + file_size), 
                        0, 
                        mem_size - file_size
                    );
                }
            };
        }
    }
    
    // Return the entry point - where execution starts
    Ok(elf.header.pt2.entry_point())
}
```

#### What PT_LOAD Segments Contain

A typical vmlinux has several PT_LOAD segments:

```
Segment 0: .text (code)
  File offset: 0x200000
  Physical addr: 0x1000000 (16MB)
  File size: 0x800000
  Mem size: 0x800000
  Flags: R-X (read, execute)

Segment 1: .data + .bss
  File offset: 0xA00000
  Physical addr: 0x1800000 (24MB)
  File size: 0x100000
  Mem size: 0x200000   ← Larger! BSS needs to be zeroed
  Flags: RW- (read, write)
```

#### Why mem_size > file_size?

The BSS section contains uninitialized global variables. In the ELF file, they take no space (why store zeros?). But in memory, they need actual space initialized to zero.

```
In ELF file:              In guest memory:
┌─────────────┐           ┌─────────────┐
│ .text       │           │ .text       │
│ (from file) │           │ (copied)    │
├─────────────┤           ├─────────────┤
│ .data       │           │ .data       │
│ (from file) │           │ (copied)    │
└─────────────┘           ├─────────────┤
                          │ .bss        │
                          │ (zeroed)    │ ← We create this!
                          └─────────────┘
```

#### Effect

After loading:
- Kernel code is at the addresses it expects
- Global variables are initialized (or zeroed for BSS)
- We have the entry point address (typically `0x1000000` for vmlinux)
- The kernel is ready to execute

---

### 6. Serial Console Emulation

#### Why We Need This

When the kernel boots, it outputs messages. Without console emulation, we'd never see them - the guest would write to "hardware" that doesn't exist.

The kernel command line `console=ttyS0` tells Linux to output to the first serial port (COM1). Serial ports are simple: write a byte to port 0x3F8, and it goes out the wire. We intercept these writes and print them to our terminal.

#### How KVM I/O Interception Works

When the guest executes an `out` instruction (write to I/O port):
1. The CPU traps to KVM
2. KVM stores information about the I/O in the `kvm_run` structure
3. KVM returns control to our VMM with exit reason `KVM_EXIT_IO`
4. We handle the I/O and call `KVM_RUN` again

#### The Code

```rust
fn handle_io(&self, kvm_run: &kvm_run) -> Result<()> {
    // Get I/O details from the kvm_run structure
    let io = unsafe { kvm_run.__bindgen_anon_1.io };
    let port = io.port;                                    // Which port?
    let is_out = io.direction as u32 == KVM_EXIT_IO_OUT;  // Read or write?
    
    // Data is at an offset within the kvm_run mmap'd region
    let data_ptr = unsafe { 
        (self.run_ptr.as_ptr() as *mut u8).add(io.data_offset as usize) 
    };

    match (port, is_out) {
        // Guest writing to COM1 data register - this is a character!
        (COM1_DATA, true) => {
            let ch = unsafe { *data_ptr } as char;
            print!("{}", ch);
            _ = std::io::stdout().flush();  // Show immediately
        }
        
        // Guest reading Line Status Register
        // Kernel checks "is the transmitter ready?" before sending
        (COM1_LSR, false) => {
            // Return: Transmitter empty (0x20) | Holding register empty (0x40)
            // This tells the kernel "yes, you can send"
            unsafe { *data_ptr = 0x60; }
        }
        
        // Port 0x61 - PC speaker / timer (legacy)
        // Some kernel code checks this; return safe value
        (0x61, false) => unsafe { *data_ptr = 0x20; },
        
        // Other UART registers - ignore writes, return 0 for reads
        (COM1_IER..=COM1_MSR, _) => {
            if !is_out {
                unsafe { *data_ptr = 0; }
            }
        }
        _ => {}
    }
    Ok(())
}
```

#### COM1 Port Layout

```
Port    Name    Description
0x3F8   DATA    Transmit/Receive buffer (we handle writes)
0x3F9   IER     Interrupt Enable Register
0x3FA   IIR     Interrupt ID / FIFO Control
0x3FB   LCR     Line Control (baud rate, parity)
0x3FC   MCR     Modem Control
0x3FD   LSR     Line Status (we report "ready")
0x3FE   MSR     Modem Status
```

#### Why Return 0x60 for LSR?

When the kernel wants to send a character, it first checks the Line Status Register:

```c
// In Linux serial driver (simplified)
while (!(inb(COM1_LSR) & 0x60))
    ; // Wait until transmitter is ready
outb(character, COM1_DATA);
```

- Bit 5 (0x20): Transmitter holding register empty
- Bit 6 (0x40): Transmitter empty

By returning 0x60, we say "yes, ready to transmit!" Without this, the kernel spins forever waiting.

#### Effect

With serial emulation:
- Kernel boot messages appear on your terminal
- `printk()` output is visible
- You can see panic messages if something goes wrong
- The command `earlyprintk=ttyS0` shows even earlier boot output

Example output:
```
[    0.000000] Linux version 6.1.0...
[    0.000000] Command line: console=ttyS0 earlyprintk=ttyS0
[    0.000000] x86/fpu: x87 FPU will use FXSAVE
...
```

---

### 7. The VM Run Loop and Initial Registers

#### Setting Initial Registers

Before entering the VM, we set the starting CPU state:

```rust
fn init_registers(&self, rip: __u64, rsi: __u64) -> Result<()> {
    let mut regs = kvm_regs::default();
    unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_REGS, &mut regs, 0) };

    regs.rflags = 2;   // Bit 1 must always be set (reserved)
    regs.rip = rip;    // Instruction pointer = kernel entry point
    regs.rsi = rsi;    // RSI = pointer to boot_params (Linux boot protocol)

    unsafe { ioctl(self.fd.as_raw_fd(), KVM_SET_REGS, &mut regs, 0) };
    Ok(())
}
```

#### Why RSI Points to boot_params

The Linux 64-bit boot protocol specifies:
- **RIP**: Entry point of the kernel
- **RSI**: Pointer to the `boot_params` structure
- Other registers: Undefined (kernel doesn't rely on them)

This is documented in the kernel source at `Documentation/x86/boot.rst`.

#### The Run Loop

```rust
fn run(&self) -> Result<()> {
    loop {
        // Enter guest mode - CPU executes guest code until something happens
        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_RUN, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error())
                .context("KVM_RUN failed");
        }
        
        // Examine why we exited
        let kvm_run = unsafe { self.run_ptr.as_ref() };

        match kvm_run.exit_reason {
            KVM_EXIT_IO => {
                // Guest did port I/O - handle it
                self.handle_io(kvm_run)?;
            }
            KVM_EXIT_HLT => {
                // Guest executed HLT instruction
                // This means the kernel is done (or idle)
                println!("Guest executed HLT. Stopping.");
                break Ok(());
            }
            other => {
                // Unknown exit - for debugging, we could dump state here
            }
        };
    }
}
```

#### What Happens During KVM_RUN

```
VMM calls KVM_RUN
        │
        ▼
┌───────────────────┐
│ KVM prepares VMCS │ (Virtual Machine Control Structure)
│ loads guest state │
└─────────┬─────────┘
          │
          ▼
    ┌───────────┐
    │ VM Entry  │ ── CPU switches to guest mode
    └─────┬─────┘
          │
          ▼
   Guest code runs directly on CPU (near-native speed)
          │
          │ (I/O instruction, interrupt, etc.)
          ▼
    ┌───────────┐
    │ VM Exit   │ ── CPU switches back to host mode
    └─────┬─────┘
          │
          ▼
┌───────────────────┐
│ KVM saves guest   │
│ state, returns    │
│ to VMM            │
└─────────┬─────────┘
          │
          ▼
VMM examines exit_reason and handles it
```

#### Effect

The run loop is where the magic happens:
- Guest code executes at near-native speed
- We only get control when the guest does something we need to handle
- Serial output appears character by character as the kernel prints
- When Linux finishes booting (or panics), we see it and can exit

---

## Guest Memory Layout Summary

```
┌────────────────────────────────────────┐ 0x0
│       Interrupt Vector Table           │ (not used in our minimal setup)
├────────────────────────────────────────┤ 0x500
│       GDT (Global Descriptor Table)    │ 24 bytes (3 entries × 8 bytes)
├────────────────────────────────────────┤ 0x2000
│       PML4 (Page Map Level 4)          │ 4KB, 1 entry used
├────────────────────────────────────────┤ 0x3000
│       PDPT (Page Directory Ptr Table)  │ 4KB, 1 entry used
├────────────────────────────────────────┤ 0x4000
│       PD (Page Directory)              │ 4KB, 512 entries (1GB mapped)
├────────────────────────────────────────┤ 0x10000 (64KB)
│       boot_params structure            │ ~4KB
├────────────────────────────────────────┤ 0x20000 (128KB)
│       Kernel command line              │ "console=ttyS0..."
├────────────────────────────────────────┤ 0x100000 (1MB)
│                                        │
│       Linux Kernel Image               │
│       (loaded from ELF segments)       │
│       - .text (code)                   │
│       - .rodata (constants)            │
│       - .data (initialized globals)    │
│       - .bss (zeroed globals)          │
│                                        │
├────────────────────────────────────────┤
│                                        │
│       Free RAM                         │
│       (kernel allocates from here)     │
│                                        │
└────────────────────────────────────────┘ 2GB
```

## Running the VMM

```bash
# Build the project
cargo build --release

# Run with a vmlinux kernel
./target/release/rust-kvm-tool /path/to/vmlinux
```

You should see Linux boot messages streaming to your terminal via the emulated serial console!

## Challenges and Lessons Learned

### 1. Debugging VM Entry Failures

When the CPU fails to enter the guest, KVM provides cryptic error codes. I spent considerable time debugging issues with:
- Incorrect segment descriptor formats
- Missing page table entries
- Wrong control register settings

The key was dumping CPU state (CR0, CR3, CR4, EFER, segment registers) after each failure.

### 2. Understanding the Linux Boot Protocol

The [Linux x86 boot protocol](https://www.kernel.org/doc/html/latest/arch/x86/boot.html) is well-documented but has many subtleties. Key requirements:
- `boot_params` must be at a specific location
- RSI must point to `boot_params`
- E820 memory map must be accurate
- Certain header fields are mandatory

### 3. Page Table Complexity

Setting up page tables correctly was surprisingly tricky. The identity mapping must be in place before the kernel starts, and the flags on each entry must be correct.

## What's Missing (Future Work)

This is a minimal VMM - many features are not implemented:

- **Block devices**: No disk support (kernel panics looking for root filesystem)
- **Network**: No virtio-net or other network device
- **Multiple vCPUs**: Only single-threaded execution
- **Interrupts**: Basic IRQ chip only, no MSI/MSI-X
- **Device passthrough**: No VFIO support
- **Live migration**: No state serialization

## Conclusion

Building a VMM from scratch is an excellent way to understand:
- How hardware virtualization works
- The x86 architecture (segments, page tables, control registers)
- The Linux boot process
- Low-level systems programming in Rust

The full source code is available in this repository. Feel free to explore, experiment, and extend it!

## References

- [KVM API Documentation](https://www.kernel.org/doc/html/latest/virt/kvm/api.html)
- [Linux x86 Boot Protocol](https://www.kernel.org/doc/html/latest/arch/x86/boot.html)
- [Intel Software Developer's Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev Wiki - Setting Up Long Mode](https://wiki.osdev.org/Setting_Up_Long_Mode)
- [rust-vmm Project](https://github.com/rust-vmm) - Production-quality Rust VMM components

---

*Built with Rust, curiosity, and lots of debugging.*
