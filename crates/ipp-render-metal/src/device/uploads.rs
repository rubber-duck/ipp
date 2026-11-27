use super::*;
use std::ptr::NonNull;

// Metal copies up to 4 KiB per setVertexBytes or setFragmentBytes call.
pub(super) const INLINE_LIMIT: usize = 4096;

#[derive(Default)]
pub(super) struct Uploads {
    pub(super) used: Vec<Buffer>,
    end: usize,
}

pub(super) fn aligned(n: usize) -> usize {
    n.next_multiple_of(256)
}

impl Uploads {
    pub(super) fn reserve(&mut self, dev: &Backend, bytes: usize) -> Option<(Buffer, usize)> {
        let len = aligned(bytes);
        if let Some(last) = self.used.last()
            && len <= last.length() - self.end
        {
            let offset = self.end;
            self.end += len;
            return Some((last.clone(), offset));
        }
        let capacity = len.max(256 * 1024);
        let mut pool = dev.pool.borrow_mut();
        let buffer = if let Some(i) = pool.iter().position(|b| b.length() >= capacity) {
            pool.swap_remove(i)
        } else {
            dev.gpu.newBufferWithLength_options(
                capacity,
                MTLResourceOptions::StorageModeShared
                    | MTLResourceOptions::CPUCacheModeWriteCombined,
            )?
        };
        self.end = len;
        self.used.push(buffer.clone());
        Some((buffer, 0))
    }
}

// Only called on fresh recording storage or disjoint frame reservations that have retired on the GPU.
pub(super) fn write(buffer: &Buffer, offset: usize, data: &[u8]) {
    assert!(offset <= buffer.length() && data.len() <= buffer.length() - offset);
    // SAFETY: Callers own this unused range of the shared buffer. Its contents and the source are disjoint and live for the copy.
    unsafe {
        buffer
            .contents()
            .cast::<u8>()
            .as_ptr()
            .add(offset)
            .copy_from_nonoverlapping(data.as_ptr(), data.len())
    };
}

pub(super) fn vertex(e: &Encoder, slot: usize, b: &Buffer, offset: usize) {
    // SAFETY: Callers bind a valid range for the pipeline's declared slot. Metal retains the buffer until execution completes.
    unsafe { e.setVertexBuffer_offset_atIndex(Some(b), offset, slot) };
}

pub(super) fn fragment(e: &Encoder, slot: usize, b: &Buffer, offset: usize) {
    // SAFETY: Callers bind a valid range for the pipeline's declared slot. Metal retains the buffer until execution completes.
    unsafe { e.setFragmentBuffer_offset_atIndex(Some(b), offset, slot) };
}

pub(super) fn inline(e: &Encoder, vertex: bool, slot: usize, bytes: &[u8]) {
    assert!(bytes.len() <= INLINE_LIMIT);
    let ptr = NonNull::from_ref(bytes.first().expect("nonempty shader binding")).cast();
    // SAFETY: Metal copies this initialized slice during encoding. The slot and size match the active shader.
    unsafe {
        if vertex {
            e.setVertexBytes_length_atIndex(ptr, bytes.len(), slot)
        } else {
            e.setFragmentBytes_length_atIndex(ptr, bytes.len(), slot)
        }
    }
}

pub(super) fn texture(e: &Encoder, slot: usize, t: &Texture) {
    // SAFETY: The texture belongs to this device and is retained by this encoder's command buffer.
    unsafe { e.setFragmentTexture_atIndex(Some(t), slot) };
}
