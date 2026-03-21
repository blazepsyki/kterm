// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod renderer;

use iced::widget::image;

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
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub handle: Option<image::Handle>,
}

impl RemoteDisplayState {
    pub fn new(width: u16, height: u16) -> Self {
        let len = width as usize * height as usize * 4;
        Self {
            width,
            height,
            rgba: vec![0; len],
            handle: None,
        }
    }

    pub fn apply(&mut self, update: FrameUpdate) {
        match update {
            FrameUpdate::Full { width, height, rgba } => {
                self.width = width;
                self.height = height;
                self.rgba = rgba;
                self.handle = Some(renderer::build_rgba_handle(self.width, self.height, self.rgba.clone()));
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

                    if dst_end <= self.rgba.len() {
                        self.rgba[dst_start..dst_end].copy_from_slice(&rgba[src_start..src_end]);
                    }
                }

                self.handle = Some(renderer::build_rgba_handle(self.width, self.height, self.rgba.clone()));
            }
        }
    }
}
