#[path = "src/icon.rs"]
mod icon;

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

fn encode_ico_entry_bmp(size: u32) -> Vec<u8> {
    let rgba = icon::render_stream_timer_rgba(size);
    let pixel_bytes = (size * size * 4) as usize;
    let mask_stride = (size as usize).div_ceil(32) * 4;
    let mask_bytes = mask_stride * (size as usize);

    let mut buf = Vec::with_capacity(40 + pixel_bytes + mask_bytes);
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(size as i32).to_le_bytes());
    buf.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&32u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            buf.push(rgba[idx + 2]);
            buf.push(rgba[idx + 1]);
            buf.push(rgba[idx]);
            buf.push(rgba[idx + 3]);
        }
    }
    buf.resize(buf.len() + mask_bytes, 0u8);
    buf
}

fn write_multi_res_ico(path: &PathBuf) -> io::Result<()> {
    let sizes: &[u32] = &[16, 24, 32, 48, 64, 128, 256];
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&s| (s, encode_ico_entry_bmp(s)))
        .collect();

    let mut file = File::create(path)?;
    file.write_all(&0u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&(images.len() as u16).to_le_bytes())?;

    let mut offset = 6u32 + (images.len() as u32) * 16u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        file.write_all(&[dim, dim, 0, 0])?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&32u16.to_le_bytes())?;
        file.write_all(&(data.len() as u32).to_le_bytes())?;
        file.write_all(&offset.to_le_bytes())?;
        offset += data.len() as u32;
    }

    for (_, data) in &images {
        file.write_all(data)?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        let ico_path = out_dir.join("stream_timer.ico");
        write_multi_res_ico(&ico_path)?;

        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("valid UTF-8 ico path"));
        res.set("ProductName", "Stream Timer");
        res.set("FileDescription", "Minimalist Live Stream Timer (00:00:00)");
        res.set("OriginalFilename", "stream_timer.exe");
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>"#,
        );
        res.compile()?;
    }

    Ok(())
}
