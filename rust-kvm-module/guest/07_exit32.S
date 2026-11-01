# Stage 7 guest: 32-bit protected mode, flat, paging off (so guest-physical == host-physical
# and no EPT is needed yet). Position independent: stage 7's selftest copies it into a
# page and starts the vCPU at that page's physical address.
# Exits taken: 2x CPUID, 3x OUT (port 0x3f8), HLT.
# Expected afterwards: ESI = CPUID.1:ECX (VMX hidden, hypervisor bit set),
#                      EDI = CPUID.0x40000000:EBX = "KtVm"; console = "Hi\n".
	.code32
	.globl _start
_start:
	mov	$1, %eax
	cpuid
	mov	%ecx, %esi
	mov	$0x40000000, %eax
	cpuid
	mov	%ebx, %edi
	mov	$0x3f8, %edx
	mov	$'H', %al
	out	%al, %dx
	mov	$'i', %al
	out	%al, %dx
	mov	$10, %al
	out	%al, %dx
	hlt
