//! Constants, mirroring arch/x86/include/asm/vmx.h, arch/x86/include/asm/msr-index.h
//! and arch/x86/include/asm/vmxfeatures.h. PROVIDED: nothing to implement here.
//! Book: Intel SDM Vol. 3C, Appendix A (capability MSRs), B (field encodings),
//! C (basic exit reasons), and chapters 24-27.

// ---- MSRs (SDM Vol 4 / Vol 3C App. A) ------------------------------------------
pub const MSR_IA32_FEATURE_CONTROL: u32 = 0x3a;
pub const MSR_IA32_VMX_BASIC: u32 = 0x480;
pub const MSR_IA32_VMX_PINBASED_CTLS: u32 = 0x481;
pub const MSR_IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
pub const MSR_IA32_VMX_EXIT_CTLS: u32 = 0x483;
pub const MSR_IA32_VMX_ENTRY_CTLS: u32 = 0x484;
pub const MSR_IA32_VMX_MISC: u32 = 0x485;
pub const MSR_IA32_VMX_CR0_FIXED0: u32 = 0x486;
pub const MSR_IA32_VMX_CR0_FIXED1: u32 = 0x487;
pub const MSR_IA32_VMX_CR4_FIXED0: u32 = 0x488;
pub const MSR_IA32_VMX_CR4_FIXED1: u32 = 0x489;
pub const MSR_IA32_VMX_VMCS_ENUM: u32 = 0x48a;
pub const MSR_IA32_VMX_PROCBASED_CTLS2: u32 = 0x48b;
pub const MSR_IA32_VMX_EPT_VPID_CAP: u32 = 0x48c;
pub const MSR_IA32_VMX_TRUE_PINBASED_CTLS: u32 = 0x48d;
pub const MSR_IA32_VMX_TRUE_PROCBASED_CTLS: u32 = 0x48e;
pub const MSR_IA32_VMX_TRUE_EXIT_CTLS: u32 = 0x48f;
pub const MSR_IA32_VMX_TRUE_ENTRY_CTLS: u32 = 0x490;

pub const MSR_EFER: u32 = 0xc000_0080;
pub const MSR_FS_BASE: u32 = 0xc000_0100;
pub const MSR_GS_BASE: u32 = 0xc000_0101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xc000_0102;
pub const MSR_IA32_SYSENTER_CS: u32 = 0x174;
pub const MSR_IA32_SYSENTER_ESP: u32 = 0x175;
pub const MSR_IA32_SYSENTER_EIP: u32 = 0x176;
pub const MSR_IA32_CR_PAT: u32 = 0x277;
pub const MSR_IA32_DEBUGCTLMSR: u32 = 0x1d9;

// IA32_FEATURE_CONTROL bits
pub const FEAT_CTL_LOCKED: u64 = 1 << 0;
pub const FEAT_CTL_VMX_ENABLED_INSIDE_SMX: u64 = 1 << 1;
pub const FEAT_CTL_VMX_ENABLED_OUTSIDE_SMX: u64 = 1 << 2;

// EFER bits
pub const EFER_SCE: u64 = 1 << 0;
pub const EFER_LME: u64 = 1 << 8;
pub const EFER_LMA: u64 = 1 << 10;
pub const EFER_NX: u64 = 1 << 11;

// CR0 / CR4 / RFLAGS bits
pub const X86_CR0_PE: u64 = 1 << 0;
pub const X86_CR0_MP: u64 = 1 << 1;
pub const X86_CR0_ET: u64 = 1 << 4;
pub const X86_CR0_NE: u64 = 1 << 5;
pub const X86_CR0_WP: u64 = 1 << 16;
pub const X86_CR0_PG: u64 = 1 << 31;
pub const X86_CR4_PSE: u64 = 1 << 4;
pub const X86_CR4_PAE: u64 = 1 << 5;
pub const X86_CR4_VMXE: u64 = 1 << 13;
pub const X86_EFLAGS_CF: u64 = 1 << 0;
pub const X86_EFLAGS_FIXED: u64 = 1 << 1;
pub const X86_EFLAGS_ZF: u64 = 1 << 6;
pub const X86_EFLAGS_IF: u64 = 1 << 9;
pub const X86_EFLAGS_VM: u64 = 1 << 17;

// CPUID.1:ECX
pub const CPUID_1_ECX_VMX: u32 = 1 << 5;
pub const CPUID_1_ECX_HYPERVISOR: u32 = 1 << 31;

// ---- VMCS control bits (asm/vmx.h) ---------------------------------------------
// Pin-based
pub const PIN_BASED_EXT_INTR_MASK: u32 = 1 << 0;
pub const PIN_BASED_NMI_EXITING: u32 = 1 << 3;
pub const PIN_BASED_VIRTUAL_NMIS: u32 = 1 << 5;
pub const PIN_BASED_VMX_PREEMPTION_TIMER: u32 = 1 << 6;
pub const PIN_BASED_POSTED_INTR: u32 = 1 << 7;
// Primary processor-based
pub const CPU_BASED_INTR_WINDOW_EXITING: u32 = 1 << 2;
pub const CPU_BASED_USE_TSC_OFFSETTING: u32 = 1 << 3;
pub const CPU_BASED_HLT_EXITING: u32 = 1 << 7;
pub const CPU_BASED_INVLPG_EXITING: u32 = 1 << 9;
pub const CPU_BASED_MWAIT_EXITING: u32 = 1 << 10;
pub const CPU_BASED_RDPMC_EXITING: u32 = 1 << 11;
pub const CPU_BASED_RDTSC_EXITING: u32 = 1 << 12;
pub const CPU_BASED_CR3_LOAD_EXITING: u32 = 1 << 15;
pub const CPU_BASED_CR3_STORE_EXITING: u32 = 1 << 16;
pub const CPU_BASED_CR8_LOAD_EXITING: u32 = 1 << 19;
pub const CPU_BASED_CR8_STORE_EXITING: u32 = 1 << 20;
pub const CPU_BASED_TPR_SHADOW: u32 = 1 << 21;
pub const CPU_BASED_NMI_WINDOW_EXITING: u32 = 1 << 22;
pub const CPU_BASED_MOV_DR_EXITING: u32 = 1 << 23;
pub const CPU_BASED_UNCOND_IO_EXITING: u32 = 1 << 24;
pub const CPU_BASED_USE_IO_BITMAPS: u32 = 1 << 25;
pub const CPU_BASED_MONITOR_TRAP_FLAG: u32 = 1 << 27;
pub const CPU_BASED_USE_MSR_BITMAPS: u32 = 1 << 28;
pub const CPU_BASED_MONITOR_EXITING: u32 = 1 << 29;
pub const CPU_BASED_PAUSE_EXITING: u32 = 1 << 30;
pub const CPU_BASED_ACTIVATE_SECONDARY_CONTROLS: u32 = 1 << 31;
// Secondary processor-based
pub const SECONDARY_EXEC_VIRTUALIZE_APIC_ACCESSES: u32 = 1 << 0;
pub const SECONDARY_EXEC_ENABLE_EPT: u32 = 1 << 1;
pub const SECONDARY_EXEC_DESC: u32 = 1 << 2;
pub const SECONDARY_EXEC_ENABLE_RDTSCP: u32 = 1 << 3;
pub const SECONDARY_EXEC_ENABLE_VPID: u32 = 1 << 5;
pub const SECONDARY_EXEC_WBINVD_EXITING: u32 = 1 << 6;
pub const SECONDARY_EXEC_UNRESTRICTED_GUEST: u32 = 1 << 7;
pub const SECONDARY_EXEC_ENABLE_INVPCID: u32 = 1 << 12;
pub const SECONDARY_EXEC_ENABLE_XSAVES: u32 = 1 << 20;
// VM-exit controls
pub const VM_EXIT_SAVE_DEBUG_CONTROLS: u32 = 1 << 2;
pub const VM_EXIT_HOST_ADDR_SPACE_SIZE: u32 = 1 << 9;
pub const VM_EXIT_ACK_INTR_ON_EXIT: u32 = 1 << 15;
pub const VM_EXIT_SAVE_IA32_PAT: u32 = 1 << 18;
pub const VM_EXIT_LOAD_IA32_PAT: u32 = 1 << 19;
pub const VM_EXIT_SAVE_IA32_EFER: u32 = 1 << 20;
pub const VM_EXIT_LOAD_IA32_EFER: u32 = 1 << 21;
pub const VM_EXIT_SAVE_VMX_PREEMPTION_TIMER: u32 = 1 << 22;
// VM-entry controls
pub const VM_ENTRY_LOAD_DEBUG_CONTROLS: u32 = 1 << 2;
pub const VM_ENTRY_IA32E_MODE: u32 = 1 << 9;
pub const VM_ENTRY_SMM: u32 = 1 << 10;
pub const VM_ENTRY_LOAD_IA32_PAT: u32 = 1 << 14;
pub const VM_ENTRY_LOAD_IA32_EFER: u32 = 1 << 15;

// ---- VMCS field encodings (SDM App. B; enum vmcs_field in asm/vmx.h) ----------
pub const VIRTUAL_PROCESSOR_ID: u32 = 0x0000_0000;
pub const GUEST_ES_SELECTOR: u32 = 0x0000_0800;
pub const GUEST_CS_SELECTOR: u32 = 0x0000_0802;
pub const GUEST_SS_SELECTOR: u32 = 0x0000_0804;
pub const GUEST_DS_SELECTOR: u32 = 0x0000_0806;
pub const GUEST_FS_SELECTOR: u32 = 0x0000_0808;
pub const GUEST_GS_SELECTOR: u32 = 0x0000_080a;
pub const GUEST_LDTR_SELECTOR: u32 = 0x0000_080c;
pub const GUEST_TR_SELECTOR: u32 = 0x0000_080e;
pub const HOST_ES_SELECTOR: u32 = 0x0000_0c00;
pub const HOST_CS_SELECTOR: u32 = 0x0000_0c02;
pub const HOST_SS_SELECTOR: u32 = 0x0000_0c04;
pub const HOST_DS_SELECTOR: u32 = 0x0000_0c06;
pub const HOST_FS_SELECTOR: u32 = 0x0000_0c08;
pub const HOST_GS_SELECTOR: u32 = 0x0000_0c0a;
pub const HOST_TR_SELECTOR: u32 = 0x0000_0c0c;
pub const IO_BITMAP_A: u32 = 0x0000_2000;
pub const IO_BITMAP_B: u32 = 0x0000_2002;
pub const MSR_BITMAP: u32 = 0x0000_2004;
pub const VM_EXIT_MSR_STORE_ADDR: u32 = 0x0000_2006;
pub const VM_EXIT_MSR_LOAD_ADDR: u32 = 0x0000_2008;
pub const VM_ENTRY_MSR_LOAD_ADDR: u32 = 0x0000_200a;
pub const TSC_OFFSET: u32 = 0x0000_2010;
pub const EPT_POINTER: u32 = 0x0000_201a;
pub const GUEST_PHYSICAL_ADDRESS: u32 = 0x0000_2400;
pub const VMCS_LINK_POINTER: u32 = 0x0000_2800;
pub const GUEST_IA32_DEBUGCTL: u32 = 0x0000_2802;
pub const GUEST_IA32_PAT: u32 = 0x0000_2804;
pub const GUEST_IA32_EFER: u32 = 0x0000_2806;
pub const HOST_IA32_PAT: u32 = 0x0000_2c00;
pub const HOST_IA32_EFER: u32 = 0x0000_2c02;
pub const PIN_BASED_VM_EXEC_CONTROL: u32 = 0x0000_4000;
pub const CPU_BASED_VM_EXEC_CONTROL: u32 = 0x0000_4002;
pub const EXCEPTION_BITMAP: u32 = 0x0000_4004;
pub const PAGE_FAULT_ERROR_CODE_MASK: u32 = 0x0000_4006;
pub const PAGE_FAULT_ERROR_CODE_MATCH: u32 = 0x0000_4008;
pub const CR3_TARGET_COUNT: u32 = 0x0000_400a;
pub const VM_EXIT_CONTROLS: u32 = 0x0000_400c;
pub const VM_EXIT_MSR_STORE_COUNT: u32 = 0x0000_400e;
pub const VM_EXIT_MSR_LOAD_COUNT: u32 = 0x0000_4010;
pub const VM_ENTRY_CONTROLS: u32 = 0x0000_4012;
pub const VM_ENTRY_MSR_LOAD_COUNT: u32 = 0x0000_4014;
pub const VM_ENTRY_INTR_INFO_FIELD: u32 = 0x0000_4016;
pub const VM_ENTRY_EXCEPTION_ERROR_CODE: u32 = 0x0000_4018;
pub const VM_ENTRY_INSTRUCTION_LEN: u32 = 0x0000_401a;
pub const SECONDARY_VM_EXEC_CONTROL: u32 = 0x0000_401e;
pub const VM_INSTRUCTION_ERROR: u32 = 0x0000_4400;
pub const VM_EXIT_REASON: u32 = 0x0000_4402;
pub const VM_EXIT_INTR_INFO: u32 = 0x0000_4404;
pub const VM_EXIT_INTR_ERROR_CODE: u32 = 0x0000_4406;
pub const IDT_VECTORING_INFO_FIELD: u32 = 0x0000_4408;
pub const IDT_VECTORING_ERROR_CODE: u32 = 0x0000_440a;
pub const VM_EXIT_INSTRUCTION_LEN: u32 = 0x0000_440c;
pub const VMX_INSTRUCTION_INFO: u32 = 0x0000_440e;
pub const GUEST_ES_LIMIT: u32 = 0x0000_4800;
pub const GUEST_CS_LIMIT: u32 = 0x0000_4802;
pub const GUEST_SS_LIMIT: u32 = 0x0000_4804;
pub const GUEST_DS_LIMIT: u32 = 0x0000_4806;
pub const GUEST_FS_LIMIT: u32 = 0x0000_4808;
pub const GUEST_GS_LIMIT: u32 = 0x0000_480a;
pub const GUEST_LDTR_LIMIT: u32 = 0x0000_480c;
pub const GUEST_TR_LIMIT: u32 = 0x0000_480e;
pub const GUEST_GDTR_LIMIT: u32 = 0x0000_4810;
pub const GUEST_IDTR_LIMIT: u32 = 0x0000_4812;
pub const GUEST_ES_AR_BYTES: u32 = 0x0000_4814;
pub const GUEST_CS_AR_BYTES: u32 = 0x0000_4816;
pub const GUEST_SS_AR_BYTES: u32 = 0x0000_4818;
pub const GUEST_DS_AR_BYTES: u32 = 0x0000_481a;
pub const GUEST_FS_AR_BYTES: u32 = 0x0000_481c;
pub const GUEST_GS_AR_BYTES: u32 = 0x0000_481e;
pub const GUEST_LDTR_AR_BYTES: u32 = 0x0000_4820;
pub const GUEST_TR_AR_BYTES: u32 = 0x0000_4822;
pub const GUEST_INTERRUPTIBILITY_INFO: u32 = 0x0000_4824;
pub const GUEST_ACTIVITY_STATE: u32 = 0x0000_4826;
pub const GUEST_SYSENTER_CS: u32 = 0x0000_482a;
pub const VMX_PREEMPTION_TIMER_VALUE: u32 = 0x0000_482e;
pub const HOST_IA32_SYSENTER_CS: u32 = 0x0000_4c00;
pub const CR0_GUEST_HOST_MASK: u32 = 0x0000_6000;
pub const CR4_GUEST_HOST_MASK: u32 = 0x0000_6002;
pub const CR0_READ_SHADOW: u32 = 0x0000_6004;
pub const CR4_READ_SHADOW: u32 = 0x0000_6006;
pub const EXIT_QUALIFICATION: u32 = 0x0000_6400;
pub const GUEST_LINEAR_ADDRESS: u32 = 0x0000_640a;
pub const GUEST_CR0: u32 = 0x0000_6800;
pub const GUEST_CR3: u32 = 0x0000_6802;
pub const GUEST_CR4: u32 = 0x0000_6804;
pub const GUEST_ES_BASE: u32 = 0x0000_6806;
pub const GUEST_CS_BASE: u32 = 0x0000_6808;
pub const GUEST_SS_BASE: u32 = 0x0000_680a;
pub const GUEST_DS_BASE: u32 = 0x0000_680c;
pub const GUEST_FS_BASE: u32 = 0x0000_680e;
pub const GUEST_GS_BASE: u32 = 0x0000_6810;
pub const GUEST_LDTR_BASE: u32 = 0x0000_6812;
pub const GUEST_TR_BASE: u32 = 0x0000_6814;
pub const GUEST_GDTR_BASE: u32 = 0x0000_6816;
pub const GUEST_IDTR_BASE: u32 = 0x0000_6818;
pub const GUEST_DR7: u32 = 0x0000_681a;
pub const GUEST_RSP: u32 = 0x0000_681c;
pub const GUEST_RIP: u32 = 0x0000_681e;
pub const GUEST_RFLAGS: u32 = 0x0000_6820;
pub const GUEST_PENDING_DBG_EXCEPTIONS: u32 = 0x0000_6822;
pub const GUEST_SYSENTER_ESP: u32 = 0x0000_6824;
pub const GUEST_SYSENTER_EIP: u32 = 0x0000_6826;
pub const HOST_CR0: u32 = 0x0000_6c00;
pub const HOST_CR3: u32 = 0x0000_6c02;
pub const HOST_CR4: u32 = 0x0000_6c04;
pub const HOST_FS_BASE: u32 = 0x0000_6c06;
pub const HOST_GS_BASE: u32 = 0x0000_6c08;
pub const HOST_TR_BASE: u32 = 0x0000_6c0a;
pub const HOST_GDTR_BASE: u32 = 0x0000_6c0c;
pub const HOST_IDTR_BASE: u32 = 0x0000_6c0e;
pub const HOST_IA32_SYSENTER_ESP: u32 = 0x0000_6c10;
pub const HOST_IA32_SYSENTER_EIP: u32 = 0x0000_6c12;
pub const HOST_RSP: u32 = 0x0000_6c14;
pub const HOST_RIP: u32 = 0x0000_6c16;

// ---- Basic exit reasons (SDM App. C; asm/vmx.h EXIT_REASON_*) -----------------
pub const EXIT_REASON_EXCEPTION_NMI: u32 = 0;
pub const EXIT_REASON_EXTERNAL_INTERRUPT: u32 = 1;
pub const EXIT_REASON_TRIPLE_FAULT: u32 = 2;
pub const EXIT_REASON_INTERRUPT_WINDOW: u32 = 7;
pub const EXIT_REASON_NMI_WINDOW: u32 = 8;
pub const EXIT_REASON_TASK_SWITCH: u32 = 9;
pub const EXIT_REASON_CPUID: u32 = 10;
pub const EXIT_REASON_HLT: u32 = 12;
pub const EXIT_REASON_INVD: u32 = 13;
pub const EXIT_REASON_INVLPG: u32 = 14;
pub const EXIT_REASON_RDPMC: u32 = 15;
pub const EXIT_REASON_RDTSC: u32 = 16;
pub const EXIT_REASON_VMCALL: u32 = 18;
pub const EXIT_REASON_VMCLEAR: u32 = 19;
pub const EXIT_REASON_VMLAUNCH: u32 = 20;
pub const EXIT_REASON_VMPTRLD: u32 = 21;
pub const EXIT_REASON_VMREAD: u32 = 23;
pub const EXIT_REASON_VMRESUME: u32 = 24;
pub const EXIT_REASON_VMWRITE: u32 = 25;
pub const EXIT_REASON_VMOFF: u32 = 26;
pub const EXIT_REASON_VMON: u32 = 27;
pub const EXIT_REASON_CR_ACCESS: u32 = 28;
pub const EXIT_REASON_DR_ACCESS: u32 = 29;
pub const EXIT_REASON_IO_INSTRUCTION: u32 = 30;
pub const EXIT_REASON_MSR_READ: u32 = 31;
pub const EXIT_REASON_MSR_WRITE: u32 = 32;
pub const EXIT_REASON_INVALID_STATE: u32 = 33;
pub const EXIT_REASON_MSR_LOAD_FAIL: u32 = 34;
pub const EXIT_REASON_MWAIT_INSTRUCTION: u32 = 36;
pub const EXIT_REASON_MONITOR_TRAP_FLAG: u32 = 37;
pub const EXIT_REASON_PAUSE_INSTRUCTION: u32 = 40;
pub const EXIT_REASON_MCE_DURING_VMENTRY: u32 = 41;
pub const EXIT_REASON_TPR_BELOW_THRESHOLD: u32 = 43;
pub const EXIT_REASON_APIC_ACCESS: u32 = 44;
pub const EXIT_REASON_EPT_VIOLATION: u32 = 48;
pub const EXIT_REASON_EPT_MISCONFIG: u32 = 49;
pub const EXIT_REASON_INVEPT: u32 = 50;
pub const EXIT_REASON_RDTSCP: u32 = 51;
pub const EXIT_REASON_PREEMPTION_TIMER: u32 = 52;
pub const EXIT_REASON_INVVPID: u32 = 53;
pub const EXIT_REASON_WBINVD: u32 = 54;
pub const EXIT_REASON_XSETBV: u32 = 55;
/// VM_EXIT_REASON bit 31: the exit is a failed VM entry.
pub const VMX_EXIT_REASONS_FAILED_VMENTRY: u32 = 0x8000_0000;

// ---- VM-instruction error numbers (SDM 30.4, Table 30-1) ----------------------
pub const VMXERR_VMCALL_IN_VMX_ROOT_OPERATION: u32 = 1;
pub const VMXERR_VMCLEAR_INVALID_ADDRESS: u32 = 2;
pub const VMXERR_VMCLEAR_VMXON_POINTER: u32 = 3;
pub const VMXERR_VMLAUNCH_NONCLEAR_VMCS: u32 = 4;
pub const VMXERR_VMRESUME_NONLAUNCHED_VMCS: u32 = 5;
pub const VMXERR_VMRESUME_AFTER_VMXOFF: u32 = 6;
pub const VMXERR_ENTRY_INVALID_CONTROL_FIELD: u32 = 7;
pub const VMXERR_ENTRY_INVALID_HOST_STATE_FIELD: u32 = 8;
pub const VMXERR_VMPTRLD_INVALID_ADDRESS: u32 = 9;
pub const VMXERR_VMPTRLD_VMXON_POINTER: u32 = 10;
pub const VMXERR_VMPTRLD_INCORRECT_VMCS_REVISION_ID: u32 = 11;
pub const VMXERR_UNSUPPORTED_VMCS_COMPONENT: u32 = 12;
pub const VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT: u32 = 13;
pub const VMXERR_VMXON_IN_VMX_ROOT_OPERATION: u32 = 15;
pub const VMXERR_ENTRY_INVALID_EXECUTIVE_VMCS_POINTER: u32 = 16;
pub const VMXERR_ENTRY_NONLAUNCHED_EXECUTIVE_VMCS: u32 = 17;
pub const VMXERR_ENTRY_EXECUTIVE_VMCS_POINTER_NOT_VMXON_POINTER: u32 = 18;
pub const VMXERR_VMCALL_NONCLEAR_VMCS: u32 = 19;
pub const VMXERR_VMCALL_INVALID_VM_EXIT_CONTROL_FIELDS: u32 = 20;
pub const VMXERR_VMCALL_INCORRECT_MSEG_REVISION_ID: u32 = 22;
pub const VMXERR_VMXOFF_UNDER_DUAL_MONITOR_TREATMENT_OF_SMIS_AND_SMM: u32 = 23;
pub const VMXERR_VMCALL_INVALID_SMM_MONITOR_FEATURES: u32 = 24;
pub const VMXERR_ENTRY_INVALID_VM_EXECUTION_CONTROL_FIELDS_IN_EXECUTIVE_VMCS: u32 = 25;
pub const VMXERR_ENTRY_EVENTS_BLOCKED_BY_MOV_SS: u32 = 26;
pub const VMXERR_INVALID_OPERAND_TO_INVEPT_INVVPID: u32 = 28;

// ---- Event injection / interruption info (SDM 25.8.3, 27.2.2) ------------------
pub const INTR_INFO_VECTOR_MASK: u32 = 0xff;
pub const INTR_INFO_INTR_TYPE_MASK: u32 = 0x700;
pub const INTR_INFO_DELIVER_CODE_MASK: u32 = 0x800;
pub const INTR_INFO_VALID_MASK: u32 = 0x8000_0000;
pub const INTR_TYPE_EXT_INTR: u32 = 0 << 8;
pub const INTR_TYPE_NMI_INTR: u32 = 2 << 8;
pub const INTR_TYPE_HARD_EXCEPTION: u32 = 3 << 8;
pub const INTR_TYPE_SOFT_INTR: u32 = 4 << 8;
pub const INTR_TYPE_PRIV_SW_EXCEPTION: u32 = 5 << 8;
pub const INTR_TYPE_SOFT_EXCEPTION: u32 = 6 << 8;

// x86 exception vectors
pub const DE_VECTOR: u8 = 0;
pub const DB_VECTOR: u8 = 1;
pub const BP_VECTOR: u8 = 3;
pub const UD_VECTOR: u8 = 6;
pub const DF_VECTOR: u8 = 8;
pub const TS_VECTOR: u8 = 10;
pub const NP_VECTOR: u8 = 11;
pub const SS_VECTOR: u8 = 12;
pub const GP_VECTOR: u8 = 13;
pub const PF_VECTOR: u8 = 14;
pub const AC_VECTOR: u8 = 17;

// EPT (SDM 28.3)
pub const VMX_EPT_READABLE_MASK: u64 = 1 << 0;
pub const VMX_EPT_WRITABLE_MASK: u64 = 1 << 1;
pub const VMX_EPT_EXECUTABLE_MASK: u64 = 1 << 2;
pub const VMX_EPT_MT_EPTE_SHIFT: u32 = 3;
pub const VMX_EPT_IPAT_BIT: u64 = 1 << 6;
pub const VMX_EPT_PAGE_SIZE_BIT: u64 = 1 << 7;
pub const VMX_EPT_ACCESS_BIT: u64 = 1 << 8;
pub const VMX_EPT_DIRTY_BIT: u64 = 1 << 9;
pub const VMX_EPT_MT_WB: u64 = 6;
pub const VMX_EPT_MT_UC: u64 = 0;
pub const VMX_EPTP_PWL_4: u64 = 3 << 3; // page-walk length - 1, bits 5:3
pub const VMX_EPTP_AD_ENABLE_BIT: u64 = 1 << 6;
