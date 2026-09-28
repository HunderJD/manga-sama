//! The few commands of the kitty graphics protocol the reader needs:
//! <https://sw.kovidgoyal.net/kitty/graphics-protocol/>

use std::io::{Write, stdout};
use std::os::unix::ffi::OsStrExt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::Result;
use crate::pages::Image;

/// Part of an image shown 1:1: source rectangle in pixels, drawn from cell (`x`, `y`) + `offset` px down.
#[derive(PartialEq)]
pub struct Placement {
    pub id: u32,
    pub x: u16,
    pub y: u16,
    pub offset: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

/// Wraps graphics keys in the escape sequence kitty reads; `q=2` keeps kitty from answering.
fn command(keys: &str) -> String {
    format!("\x1b_Gq=2,{keys}\x1b\\")
}

/// Stores `image` in kitty under `id`, without showing it. The pixels go through a temporary
/// file that kitty deletes after reading (`t=t`), so megabytes never go through the terminal.
pub fn transmit(id: u32, image: &Image) -> Result<()> {
    let name = format!(
        "tty-graphics-protocol-manga-sama-{}-{id}",
        std::process::id()
    );
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, &image.rgb)?;
    let path = STANDARD.encode(path.as_os_str().as_bytes());
    let (width, height) = (image.width, image.height);
    send(&command(&format!(
        "a=t,t=t,f=24,s={width},v={height},i={id};{path}"
    )))
}

/// Shows part of a stored image. The placement id is always 1, so placing an image again
/// moves it instead of adding a copy. `C=1`: the cursor stays where it is.
pub fn place(p: &Placement) -> String {
    let cursor = format!("\x1b[{};{}H", p.y + 1, p.x + 1);
    let keys = format!(
        "a=p,i={},p=1,x=0,y={},w={},h={},Y={},C=1",
        p.id, p.src_y, p.src_w, p.src_h, p.offset
    );
    cursor + &command(&keys)
}

/// Hides image `id`; kitty keeps it stored.
pub fn hide(id: u32) -> String {
    command(&format!("a=d,d=i,i={id}"))
}

/// Deletes image `id` from kitty's memory.
pub fn free(id: u32) -> String {
    command(&format!("a=d,d=I,i={id}"))
}

/// Writes commands in one go, so kitty never shows a half-updated frame.
pub fn send(commands: &str) -> Result<()> {
    let mut out = stdout().lock();
    out.write_all(commands.as_bytes())?;
    out.flush()?;
    Ok(())
}
