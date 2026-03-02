use bootloader::DiskImageBuilder;
use std::path::PathBuf;

fn main() {
    let kernel_path: PathBuf = std::env::args()
        .nth(1)
        .expect("usage: runner <kernel-binary> [output-stem]")
        .into();

    // Optional stem: "oci-kernel" → oci-kernel-bios.img + oci-kernel-uefi.img
    let stem: String = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "oci-kernel".into());

    let kernel_path = kernel_path
        .canonicalize()
        .expect("kernel binary not found");

    let bios_path = PathBuf::from(format!("{}-bios.img", stem));
    let uefi_path = PathBuf::from(format!("{}-uefi.img", stem));

    println!("Building images from {:?}...", kernel_path);

    let builder = DiskImageBuilder::new(kernel_path);

    builder.create_bios_image(&bios_path)
        .expect("failed to create BIOS disk image");
    println!("  BIOS: {}", bios_path.display());

    builder.create_uefi_image(&uefi_path)
        .expect("failed to create UEFI disk image");
    println!("  UEFI: {}", uefi_path.display());

    println!("Done.");
}
