// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod renderer;

use renderer::DirtyRect;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_DISPLAY_SOURCE_ID: AtomicU64 = AtomicU64::new(1);
const FULL_UPLOAD_DIRTY_RECT_THRESHOLD: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullUploadPromotionReason {
    Bootstrap,
    LargeRectBatch,
}

impl FullUploadPromotionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::LargeRectBatch => "large_rect_batch",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameBatchStats {
    pub full_count: usize,
    pub rect_count: usize,
    pub forced_full_upload_reason: Option<FullUploadPromotionReason>,
}

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
                if self.dirty_rects.len() > FULL_UPLOAD_DIRTY_RECT_THRESHOLD {
                    self.full_upload = true;
                    self.dirty_rects.clear();
                }

                self.frame_seq = self.frame_seq.wrapping_add(1);
                self.frame_ready = true;
            }
        }
    }

    pub fn apply_batch<I>(&mut self, updates: I) -> FrameBatchStats
    where
        I: IntoIterator<Item = FrameUpdate>,
    {
        let frame_seq_before = self.frame_seq;
        let mut stats = FrameBatchStats::default();

        // Start a fresh dirty batch for this UI event.
        if !self.full_upload {
            self.dirty_rects.clear();
        }

        for update in updates {
            match &update {
                FrameUpdate::Full { .. } => stats.full_count += 1,
                FrameUpdate::Rect { .. } => stats.rect_count += 1,
            }
            self.apply(update);
        }

        let force_bootstrap = frame_seq_before == 0 && stats.full_count == 0 && stats.rect_count > 0;
        let force_large_batch = stats.full_count == 0 && stats.rect_count > FULL_UPLOAD_DIRTY_RECT_THRESHOLD;

        stats.forced_full_upload_reason = if force_bootstrap {
            Some(FullUploadPromotionReason::Bootstrap)
        } else if force_large_batch {
            Some(FullUploadPromotionReason::LargeRectBatch)
        } else {
            None
        };

        if stats.forced_full_upload_reason.is_some() {
            self.full_upload = true;
            self.dirty_rects.clear();
        }

        stats
    }

}
