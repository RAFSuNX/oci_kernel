use x86_64::{
    VirtAddr,
    structures::paging::{
        PageTableFlags, Page, FrameAllocator, Mapper, Size4KiB,
    },
};

pub const HEAP_START: usize = 0xFFFF_C000_0000_0000;
pub const HEAP_SIZE:  usize = 8 * 1024 * 1024; // 8MB

pub fn init(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end   = heap_start + HEAP_SIZE as u64 - 1u64;
        let start_page = Page::containing_address(heap_start);
        let end_page   = Page::containing_address(heap_end);
        Page::range_inclusive(start_page, end_page)
    };

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .expect("no physical frames for heap");
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .unwrap()
                .flush();
        }
    }

    unsafe {
        super::super::ALLOCATOR
            .lock()
            .init(HEAP_START as *mut u8, HEAP_SIZE);
    }
}
