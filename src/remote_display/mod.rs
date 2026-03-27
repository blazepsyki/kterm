// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod renderer;

use renderer::DirtyRect;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_DISPLAY_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub enum FrameUpdate {
    Full {
        width: u16,
        height: u16,
        rgba: Vec<u8>,
    },
    Rect {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct RemoteDisplayState {
    pub source_id: u64,
    pub width: u16,
    pub height: u16,
    pub rgba: Arc<Vec<u8>>,
    pub dirty_rects: Vec<DirtyRect>,
    pub full_upload: bool,
    pub frame_seq: u64,
    pub frame_ready: bool,
    pub status_message: Option<String>,
}

impl RemoteDisplayState {
    pub fn new(width: u16, height: u16) -> Self {
        let len = width as usize * height as usize * 4;
        Self {
            source_id: NEXT_DISPLAY_SOURCE_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            rgba: Arc::new(vec![0; len]),
            dirty_rects: Vec::new(),
            full_upload: true,
            frame_seq: 0,
            frame_ready: false,
            status_message: None,
        }
    }

    pub fn apply(&mut self, update: FrameUpdate) {
        // Get mutable access to the buffer (Arc::make_mut handles clone-on-write)
        let buf = Arc::make_mut(&mut self.rgba);

        match update {
            FrameUpdate::Full { width, height, rgba } => {
                self.width = width;
                self.height = height;
                *buf = rgba;
                self.full_upload = true;
                self.dirty_rects.clear();
                self.frame_seq = self.frame_seq.wrapping_add(1);
                self.frame_ready = true;
            }
            FrameUpdate::Rect {
                x,
                y,
                width,
                height,
                rgba,
            } => {
                if self.width == 0 || self.height == 0 {
                    return;
                }

                let max_w = self.width.saturating_sub(x);
                let max_h = self.height.saturating_sub(y);
                let rect_w = width.min(max_w);
                let rect_h = height.min(max_h);

                if rect_w == 0 || rect_h == 0 {
                    return;
                }

                let dst_stride = self.width as usize * 4;
                let row_bytes = rect_w as usize * 4;
                let expected = row_bytes * rect_h as usize;

                if rgba.len() < expected {
                    return;
                }

                for row in 0..rect_h as usize {
                    let src_start = row * row_bytes;
                    let src_end = src_start + row_bytes;

                    let dst_y = y as usize + row;
                    let dst_start = dst_y * dst_stride + x as usize * 4;
                    let dst_end = dst_start + row_bytes;

                    if dst_end <= buf.len() {
                        buf[dst_start..dst_end].copy_from_slice(&rgba[src_start..src_end]);
                    }
                }

                if self.full_upload {
                    // After an initial full frame, switch to incremental updates.
                    self.full_upload = false;
                    self.dirty_rects.clear();
                }

                self.dirty_rects.push(DirtyRect {
                    x: x as u32,
                    y: y as u32,
                    width: rect_w as u32,
                    height: rect_h as u32,
                });

                // If too many dirty rects accumulate, a full upload is cheaper.
                if self.dirty_rects.len() > 256 {
                    self.full_upload = true;
                    self.dirty_rects.clear();
                }

                self.frame_seq = self.frame_seq.wrapping_add(1);
                self.frame_ready = true;
            }
        }
    }

}
