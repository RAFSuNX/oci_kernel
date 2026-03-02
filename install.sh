#!/usr/bin/env bash
# install.sh — Write OCI Kernel to a physical drive.
# Requires root for dd.  Run via: make install
set -euo pipefail

BIOS_IMG="oci-kernel-0.1.0-bios.img"
UEFI_IMG="oci-kernel-0.1.0-uefi.img"

RED='\033[0;31m'
YEL='\033[1;33m'
GRN='\033[0;32m'
BLU='\033[0;34m'
NC='\033[0m'

echo -e "${BLU}╔══════════════════════════════════════╗${NC}"
echo -e "${BLU}║   OCI Kernel — Drive Installer       ║${NC}"
echo -e "${BLU}╚══════════════════════════════════════╝${NC}"
echo

# ── 1. Choose firmware mode ───────────────────────────────────────────────────
echo "Firmware mode:"
echo "  1) BIOS / Legacy  (${BIOS_IMG} — works on almost everything)"
echo "  2) UEFI           (${UEFI_IMG} — required on most machines made after 2012)"
echo
read -rp "Choose [1/2]: " MODE_CHOICE

case "$MODE_CHOICE" in
    1)
        IMG="$BIOS_IMG"
        MODE="BIOS"
        ;;
    2)
        IMG="$UEFI_IMG"
        MODE="UEFI"
        ;;
    *)
        echo -e "${RED}Invalid choice. Aborting.${NC}"
        exit 1
        ;;
esac

if [ ! -f "$IMG" ]; then
    echo -e "${RED}Image not found: $IMG${NC}"
    echo "Run 'make image' first."
    exit 1
fi

IMG_SIZE=$(du -h "$IMG" | cut -f1)
echo
echo -e "Image: ${GRN}${IMG}${NC}  (${IMG_SIZE})"
echo

# ── 2. Show drives ────────────────────────────────────────────────────────────
echo "Available block devices:"
echo "──────────────────────────────────────────────"
lsblk -d -o NAME,SIZE,MODEL,TRAN,HOTPLUG 2>/dev/null | grep -v "^loop" || lsblk -d
echo "──────────────────────────────────────────────"
echo
echo -e "${YEL}⚠  Target the WHOLE disk (e.g. sdb), NOT a partition (e.g. sdb1).${NC}"
echo

# ── 3. Get target drive ───────────────────────────────────────────────────────
read -rp "Target drive name (just the name, e.g. sdb): " DRIVE
DRIVE="${DRIVE#/dev/}"          # strip /dev/ if user typed it
DEV="/dev/${DRIVE}"

# Validate: must be a block device
if [ ! -b "${DEV}" ]; then
    echo -e "${RED}Error: ${DEV} is not a block device. Aborting.${NC}"
    exit 1
fi

# Validate: must not be a partition (no trailing digit after a letter)
if [[ "${DRIVE}" =~ [a-z][0-9]+$ ]]; then
    echo -e "${RED}Error: ${DEV} looks like a partition. Use the whole disk.${NC}"
    exit 1
fi

# Validate: must not be the root disk
ROOT_DEV=$(lsblk -no PKNAME "$(df / | tail -1 | awk '{print $1}')" 2>/dev/null || true)
if [ "${DRIVE}" = "${ROOT_DEV}" ]; then
    echo -e "${RED}Error: ${DEV} is the system disk. Aborting.${NC}"
    exit 1
fi

# ── 4. Show target info and warn ──────────────────────────────────────────────
echo
echo -e "${YEL}┌─────────────────────────────────────────────┐${NC}"
echo -e "${YEL}│  ⚠  WARNING: ALL DATA ON ${DEV} WILL BE ERASED  │${NC}"
echo -e "${YEL}└─────────────────────────────────────────────┘${NC}"
echo
lsblk "${DEV}" 2>/dev/null || true
echo

# ── 5. Double confirmation ────────────────────────────────────────────────────
read -rp "Type the device name again to confirm (or Ctrl-C to abort): " CONFIRM
CONFIRM="${CONFIRM#/dev/}"
if [ "${CONFIRM}" != "${DRIVE}" ]; then
    echo -e "${RED}Confirmation mismatch. Aborting.${NC}"
    exit 1
fi

# ── 6. Unmount any partitions on the target ───────────────────────────────────
echo
echo "Unmounting any mounted partitions on ${DEV}..."
for part in $(lsblk -ln -o NAME "${DEV}" | tail -n +2); do
    mountpoint -q "/dev/${part}" 2>/dev/null && sudo umount "/dev/${part}" && echo "  Unmounted /dev/${part}" || true
done

# ── 7. Write ──────────────────────────────────────────────────────────────────
echo
echo -e "Writing ${GRN}${IMG}${NC} (${MODE}) to ${DEV} ..."
echo
sudo dd if="${IMG}" of="${DEV}" bs=4M status=progress conv=fsync
sudo sync

echo
echo -e "${GRN}✓ Done! OCI Kernel (${MODE}) written to ${DEV}.${NC}"
echo "  Remove the drive and boot from it."
echo
echo "  Login credentials:"
echo "    username: root"
echo "    password: admin"
