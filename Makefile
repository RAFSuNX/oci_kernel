KERNEL     = kernel
TARGET     = x86_64-oci-kernel
STEM       = oci-kernel-0.1.0
BIOS_IMG   = $(STEM)-bios.img
UEFI_IMG   = $(STEM)-uefi.img
KERNEL_BIN = target/$(TARGET)/debug/oci-kernel

BUILD_STD = -Zbuild-std=core,compiler_builtins,alloc \
            -Zbuild-std-features=compiler-builtins-mem

# Host → guest port forwarding — QEMU only, not needed on real hardware.
# Default: forward host:8080 to kernel:80 (nginx container port).
# The kernel's smoltcp stack listens on :80 and proxies to the container.
# Override example: make qemu HOSTFWD=",hostfwd=tcp::9090-:80"
# NOTE: hostfwd syntax is  ,hostfwd=tcp::HOST_PORT-:GUEST_PORT
HOSTFWD ?= ,hostfwd=tcp::8080-:80

.PHONY: build image qemu qemu-uefi debug test install clean

build:
	cargo build --manifest-path $(KERNEL)/Cargo.toml \
	    --target kernel/$(TARGET).json $(BUILD_STD)

# Unit-test the pure-logic modules on the host target.
test:
	cargo test --manifest-path $(KERNEL)/Cargo.toml \
	    --target x86_64-unknown-linux-gnu

# Build kernel then create both BIOS and UEFI disk images.
image: build
	cargo run -p runner -- $(KERNEL_BIN) $(STEM)

# Boot BIOS image in QEMU (legacy, widest compatibility).
# Port-forward example: make qemu HOSTFWD=",hostfwd=tcp::8080-:80"
qemu: image
	qemu-system-x86_64 \
		-drive format=raw,file=$(BIOS_IMG) \
		-serial stdio \
		-m 512M \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-netdev user,id=net0$(HOSTFWD) \
		-device virtio-net-pci,netdev=net0

# Boot UEFI image in QEMU (requires OVMF firmware).
qemu-uefi: image
	qemu-system-x86_64 \
		-bios /usr/share/ovmf/x64/OVMF.4m.fd \
		-drive format=raw,file=$(UEFI_IMG) \
		-serial stdio \
		-m 512M \
		-no-reboot \
		-netdev user,id=net0$(HOSTFWD) \
		-device virtio-net-pci,netdev=net0

debug: image
	qemu-system-x86_64 \
		-drive format=raw,file=$(BIOS_IMG) \
		-serial stdio \
		-m 512M \
		-no-reboot \
		-s -S \
		-netdev user,id=net0$(HOSTFWD) \
		-device virtio-net-pci,netdev=net0

# Write to a physical drive.  Runs install.sh which has safety confirmation.
install: image
	@bash install.sh

clean:
	cargo clean --manifest-path $(KERNEL)/Cargo.toml
	rm -f $(BIOS_IMG) $(UEFI_IMG)
