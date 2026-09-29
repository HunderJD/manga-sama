//! Draws images in the terminal with the kitty graphics protocol and reads its size; used by both screens.
//! Spec: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>

use std::io::{Write, stdout};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::Relaxed;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate, window_size};
use ratatui::{DefaultTerminal, Frame};

use crate::Result;
use crate::i18n::t;
use crate::reader::page_loader::Image;

/// Terminal size in cells, and the size of a cell in pixels.
#[derive(Clone, Copy, PartialEq)]
pub struct Geo {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u32,
    pub cell_h: u32,
}

impl Geo {
    pub fn now() -> Result<Self> {
        let size = window_size()?;
        if size.columns == 0
            || size.rows == 0
            || size.width < size.columns
            || size.height < size.rows
        {
            return Err(t("kitty.no_pixels").into());
        }
        Ok(Geo {
            cols: size.columns,
            rows: size.rows,
            cell_w: u32::from(size.width / size.columns),
            cell_h: u32::from(size.height / size.rows),
        })
    }
}

/// Part of a stored image shown 1:1 (see `place`).
#[derive(PartialEq)]
pub struct Placement {
    pub id: u32,
    /// Cell where the image starts.
    pub x: u16,
    pub y: u16,
    /// Pixels right of the left of cell `x` where it really starts: narrow pages are centered.
    pub x_offset: u32,
    /// Pixels below the top of cell `y` where it really starts: pages scroll by the pixel, cells don't.
    pub y_offset: u32,
    /// Source rectangle in the image, in pixels: `src_w` × `src_h` from row `src_y`.
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

/// An image stored in kitty: its id and its size in pixels.
pub struct Stored {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// Wraps graphics keys in the escape sequence kitty reads; `q=2` keeps kitty from answering.
fn command(keys: &str) -> String {
    format!("\x1b_Gq=2,{keys}\x1b\\")
}

/// Name prefix of this process's transfer files. Kitty only deletes a file it read if its path
/// contains "tty-graphics-protocol".
fn temp_prefix() -> String {
    format!("tty-graphics-protocol-manga-sama-{}-", std::process::id())
}

/// Deletes the transfer files kitty never read (it deletes the ones it reads), e.g. when the app
/// quits before kitty got to them.
pub fn cleanup() {
    let prefix = temp_prefix();
    let Ok(files) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for file in files.flatten() {
        if file.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

/// Stores `image` in kitty, without showing it. The pixels go through a temporary file that
/// kitty deletes after reading (`t=t`), so megabytes never go through the terminal.
pub fn transmit(image: &Image) -> Result<Stored> {
    // One counter for the whole app, so covers and pages never share an id.
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    let id = NEXT_ID.fetch_add(1, Relaxed);
    let path = std::env::temp_dir().join(format!("{}{id}", temp_prefix()));
    std::fs::write(&path, &image.rgb)?;
    let path = STANDARD.encode(path.as_os_str().as_bytes());
    let (width, height) = (image.width, image.height);
    send(&command(&format!(
        "a=t,t=t,f=24,s={width},v={height},i={id};{path}"
    )))?;
    Ok(Stored { id, width, height })
}

/// Shows part of a stored image. The placement id is always 1, so placing an image again
/// moves it instead of adding a copy. `C=1`: the cursor stays where it is.
fn place(p: &Placement) -> String {
    let cursor = format!("\x1b[{};{}H", p.y + 1, p.x + 1);
    let keys = format!(
        "a=p,i={},p=1,x=0,y={},w={},h={},X={},Y={},C=1",
        p.id, p.src_y, p.src_w, p.src_h, p.x_offset, p.y_offset
    );
    cursor + &command(&keys)
}

/// Hides image `id`; kitty keeps it stored.
fn hide(id: u32) -> String {
    command(&format!("a=d,d=i,i={id}"))
}

/// Deletes these images from kitty's memory.
pub fn free(ids: impl IntoIterator<Item = u32>) -> Result<()> {
    let commands: String = ids
        .into_iter()
        .map(|id| command(&format!("a=d,d=I,i={id}")))
        .collect();
    send(&commands)
}

/// Moves kitty from the `shown` placements to the new ones, sending only what changed.
fn update(shown: &mut Vec<Placement>, placements: Vec<Placement>) -> Result<()> {
    let gone = shown
        .iter()
        .filter(|old| !placements.iter().any(|p| p.id == old.id))
        .map(|old| hide(old.id));
    let moved = placements.iter().filter(|p| !shown.contains(p)).map(place);
    let commands: String = gone.chain(moved).collect();
    if !commands.is_empty() {
        // Placing moves the cursor: save it and put it back where ratatui left it (search box).
        send(&format!("\x1b7{commands}\x1b8"))?;
    }
    *shown = placements;
    Ok(())
}

/// Draws a frame: the text with `draw`, then the images it returns, updated from `shown`.
/// Kitty shows both together (synchronized update), never a frame with only one of them.
pub fn frame(
    terminal: &mut DefaultTerminal,
    shown: &mut Vec<Placement>,
    draw: impl FnOnce(&mut Frame) -> Vec<Placement>,
) -> Result<()> {
    execute!(stdout(), BeginSynchronizedUpdate)?;
    let mut placements = Vec::new();
    terminal.draw(|frame| placements = draw(frame))?;
    update(shown, placements)?;
    execute!(stdout(), EndSynchronizedUpdate)?;
    Ok(())
}

/// Writes commands in one go, so kitty never shows a half-updated frame.
fn send(commands: &str) -> Result<()> {
    let mut out = stdout().lock();
    out.write_all(commands.as_bytes())?;
    out.flush()?;
    Ok(())
}
