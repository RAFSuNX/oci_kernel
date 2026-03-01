pub mod buddy;
pub mod heap;

use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB,
    },
};

/// A FrameAllocator that returns usable frames from the bootloader's memory map.
/// Uses region_idx + frame_offset to achieve O(1) amortized allocation
/// instead of rebuilding the iterator from scratch on every call.
pub struct BootInfoFrameAllocator {
    memory_regions: &'static MemoryRegions,
    region_idx: usize,
    frame_offset: u64,
}

impl BootInfoFrameAllocator {
    pub unsafe fn new(memory_regions: &'static MemoryRegions) -> Self {
        Self { memory_regions, region_idx: 0, frame_offset: 0 }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        loop {
            let region = self.memory_regions
                .iter()
                .filter(|r| r.kind == MemoryRegionKind::Usable)
                .nth(self.region_idx)?;

            let region_frames = (region.end - region.start) / 4096;
            if self.frame_offset < region_frames {
                let addr = region.start + self.frame_offset * 4096;
                self.frame_offset += 1;
                return Some(PhysFrame::containing_address(PhysAddr::new(addr)));
            }
            // Current region exhausted — advance to next
            self.region_idx += 1;
            self.frame_offset = 0;
        }
    }
}

/// Initialize an OffsetPageTable.
/// Safety: caller must ensure physical_memory_offset is valid
pub unsafe fn init_mapper(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    &mut *page_table_ptr
}

/// Top-level memory init: call from kernel_main with boot_info
pub fn init(boot_info: &'static bootloader_api::BootInfo) {
    let physical_memory_offset = VirtAddr::new(
        boot_info.physical_memory_offset.into_option()
            .expect("physical memory offset not provided by bootloader")
    );
    let mut mapper = unsafe { init_mapper(physical_memory_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::new(&boot_info.memory_regions)
    };
    heap::init(&mut mapper, &mut frame_allocator);
}
