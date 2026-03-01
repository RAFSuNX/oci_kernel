KERNEL = kernel
TARGET = x86_64-oci-kernel
IMAGE  = oci-kernel.img

.PHONY: build qemu debug clean

build:
	cargo build --manifest-path $(KERNEL)/Cargo.toml

image: build
	cargo bootimage --manifest-path $(KERNEL)/Cargo.toml
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
