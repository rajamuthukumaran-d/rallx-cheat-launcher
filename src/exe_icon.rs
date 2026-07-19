#![allow(dead_code)]

// Extracts the shell icon associated with an .exe so the Home screen can show
// the trainer's real icon instead of the letter/color placeholder. Every
// failure path returns None rather than an error - icon extraction is a
// cosmetic nicety, and callers fall back to the placeholder when it fails.

use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

pub fn extract_icon(path: &Path) -> Option<Image> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHFILEINFOW::default();
    unsafe {
        let result = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || info.hIcon.is_invalid() {
            return None;
        }

        let image = icon_to_image(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        image
    }
}

unsafe fn icon_to_image(hicon: HICON) -> Option<Image> {
    let mut icon_info = ICONINFO::default();
    GetIconInfo(hicon, &mut icon_info).ok()?;

    let mut bitmap = BITMAP::default();
    let bytes_written = GetObjectW(
        icon_info.hbmColor,
        size_of::<BITMAP>() as i32,
        Some(&mut bitmap as *mut BITMAP as *mut _),
    );

    let image = if bytes_written == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
        None
    } else {
        read_color_bitmap(
            icon_info.hbmColor,
            bitmap.bmWidth as u32,
            bitmap.bmHeight as u32,
        )
    };

    let _ = DeleteObject(icon_info.hbmColor);
    let _ = DeleteObject(icon_info.hbmMask);
    image
}

unsafe fn read_color_bitmap(hbitmap: HBITMAP, width: u32, height: u32) -> Option<Image> {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative height requests a top-down DIB, matching Slint's row order.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let hdc = CreateCompatibleDC(None);
    let lines = GetDIBits(
        hdc,
        hbitmap,
        0,
        height,
        Some(buffer.make_mut_bytes().as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(hdc);

    if lines == 0 {
        return None;
    }

    // GetDIBits writes BGRA bytes; Rgba8Pixel expects RGBA.
    for pixel in buffer.make_mut_bytes().chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Some(Image::from_rgba8(buffer))
}
