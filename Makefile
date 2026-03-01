KERNEL = kernel
TARGET = x86_64-oci-kernel
IMAGE  = oci-kernel.img

BUILD_STD = -Zbuild-std=core,compiler_builtins,alloc \
            -Zbuild-std-features=compiler-builtins-mem

.PHONY: build image qemu debug test clean

build:
	cargo build --manifest-path $(KERNEL)/Cargo.toml $(BUILD_STD)

# Unit-test the pure-logic modules (oci, fs) on the host target.
# We use the host target to avoid the build-std duplicate-core conflict.
test:
	cargo test --manifest-path $(KERNEL)/Cargo.toml \
	    --target x86_64-unknown-linux-gnu

image: build
	cargo bootimage --manifest-path $(KERNEL)/Cargo.toml $(BUILD_STD)
	cp $(KERNEL)/target/$(TARGET)/debug/bootimage-oci-kernel.bin $(IMAGE)

qemu: image
	qemu-system-x86_64 \
		-drive format=raw,file=$(IMAGE) \
		-serial stdio \
		-m 512M \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-netdev user,id=net0 \
		-device virtio-net-pci,netdev=net0

debug: image
	qemu-system-x86_64 \
		-drive format=raw,file=$(IMAGE) \
		-serial stdio \
		-m 512M \
		-no-reboot \
		-s -S

clean:
	cargo clean --manifest-path $(KERNEL)/Cargo.toml
	rm -f $(IMAGE)
