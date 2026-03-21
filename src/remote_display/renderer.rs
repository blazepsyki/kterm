// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::image;

pub fn build_rgba_handle(width: u16, height: u16, rgba: Vec<u8>) -> image::Handle {
    image::Handle::from_rgba(width as u32, height as u32, rgba)
}
