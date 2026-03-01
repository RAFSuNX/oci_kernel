const FRAME_SIZE: usize = 4096;
const MAX_ORDER: usize = 11;

extern crate alloc;
use alloc::vec::Vec;
use x86_64::PhysAddr;

pub struct BuddyAllocator {
    free_lists: [Vec<usize>; MAX_ORDER],
    pub total_frames: usize,
}

impl BuddyAllocator {
    pub fn new_from_frames(frames: impl Iterator<Item = usize>) -> Self {
        let mut allocator = Self {
            free_lists: core::array::from_fn(|_| Vec::new()),
            total_frames: 0,
        };
        for frame in frames {
            allocator.total_frames += 1;
            allocator.free(frame, 1);
        }
        allocator
    }

    pub fn allocate(&mut self, count: usize) -> Option<usize> {
        let order = count.next_power_of_two().trailing_zeros() as usize;
        for o in order..MAX_ORDER {
            if !self.free_lists[o].is_empty() {
                let frame = self.free_lists[o].pop().unwrap();
                // split down to requested order
                for split_order in (order..o).rev() {
                    let buddy = frame + (1 << split_order);
                    self.free_lists[split_order].push(buddy);
                }
                return Some(frame);
            }
        }
        None
    }

    pub fn free(&mut self, frame: usize, count: usize) {
        let order = count.next_power_of_two().trailing_zeros() as usize;
        let mut current = frame;
        let mut current_order = order;
        loop {
            if current_order >= MAX_ORDER {
                break;
            }
            let buddy = current ^ (1 << current_order);
            if let Some(pos) = self.free_lists[current_order].iter().position(|&f| f == buddy) {
                self.free_lists[current_order].remove(pos);
                current = current.min(buddy);
                current_order += 1;
            } else {
                break;
            }
        }
        self.free_lists[current_order].push(current);
    }

    pub fn phys_addr(&self, frame: usize) -> PhysAddr {
        PhysAddr::new((frame * FRAME_SIZE) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_allocator(frames: usize) -> BuddyAllocator {
        let mut a = BuddyAllocator {
            free_lists: core::array::from_fn(|_| Vec::new()),
            total_frames: 0,
        };
        for i in 0..frames {
            a.total_frames += 1;
            a.free(i, 1);
        }
        a
    }

    #[test]
    fn allocate_single_frame() {
        let mut buddy = make_allocator(64);
        let frame = buddy.allocate(1);
        assert!(frame.is_some());
    }

    #[test]
    fn allocate_and_free_reuses_frame() {
        let mut buddy = make_allocator(64);
        let frame = buddy.allocate(1).unwrap();
        buddy.free(frame, 1);
        let frame2 = buddy.allocate(1).unwrap();
        assert_eq!(frame, frame2);
    }

    #[test]
    fn allocate_large_block_is_aligned() {
        let mut buddy = make_allocator(1024);
        let frame = buddy.allocate(16).unwrap();
        assert_eq!(frame % 16, 0);
    }

    #[test]
    fn allocate_returns_none_when_empty() {
        let mut buddy = make_allocator(0);
        assert!(buddy.allocate(1).is_none());
    }
}
