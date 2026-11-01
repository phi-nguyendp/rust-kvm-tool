# Out-of-tree build against the Rust-enabled kernel build dir made by
# scripts/build_kernel.sh (see docs/setup.md).
KDIR ?= $(HOME)/WS/linux_ktvmx
all:
	$(MAKE) -C $(KDIR) M=$(CURDIR) LLVM=1 modules
clean:
	$(MAKE) -C $(KDIR) M=$(CURDIR) clean
