//! The unread count on the taskbar icon. Windows has no numbered badge for
//! desktop apps, so the number is drawn here into a small red circle and set
//! as the taskbar button's overlay icon. The tray icon's tooltip says it too.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use tauri::image::Image;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::settings::Settings;
use crate::state::AppState;

/// Side of the drawn icon in pixels. Windows shows overlays at 16 by 16 at
/// normal scaling, so this stays sharp up to 200%.
const SIZE: u32 = 32;
/// Mail events come in bursts while syncing; the count is taken once they
/// settle.
const SETTLE: Duration = Duration::from_millis(400);
/// Looks again now and then, for changes that come without an event.
const RECHECK: Duration = Duration::from_secs(30);
/// The red of the Windows close button, also used by the title bar.
const RED: [u8; 3] = [0xc4, 0x2b, 0x1c];

#[derive(Default)]
pub struct Badge {
    wake: Notify,
    /// Set the overlay again even when the count is the same, because
    /// Windows drops it when the taskbar button goes away (window hidden).
    again: AtomicBool,
}

/// What the badge says for `count` unread messages: nothing for none, and
/// 99+ once the number no longer fits in two digits.
pub fn label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_owned()),
    }
}

/// Keeps the badge up to date from now on.
pub fn start(app: &AppHandle) {
    app.manage(Badge::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let badge = app.state::<Badge>();
        // Gives the taskbar button time to appear before the first overlay.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut shown = None;
        loop {
            let (count, on) = {
                let state = app.state::<AppState>();
                let store = state.store.lock().unwrap();
                (
                    store.inbox_unread().unwrap_or(0),
                    Settings::load(&store).unread_badge,
                )
            };
            crate::shell::tray::set_unread(&app, count);
            let text = label(count).filter(|_| on);
            if badge.again.swap(false, Ordering::Relaxed) || shown.as_ref() != Some(&text) {
                show(&app, count, text.as_deref());
                shown = Some(text);
            }
            let _ = tokio::time::timeout(RECHECK, badge.wake.notified()).await;
            tokio::time::sleep(SETTLE).await;
        }
    });
}

/// Counts again soon, e.g. after mail arrived or was read.
pub fn changed(app: &AppHandle) {
    if let Some(badge) = app.try_state::<Badge>() {
        badge.wake.notify_one();
    }
}

/// Sets the overlay again, as the main window comes back from the tray.
pub fn refresh(app: &AppHandle) {
    if let Some(badge) = app.try_state::<Badge>() {
        badge.again.store(true, Ordering::Relaxed);
        badge.wake.notify_one();
    }
}

fn show(app: &AppHandle, count: u32, text: Option<&str>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(windows)]
    let _ = (count, window.set_overlay_icon(text.map(draw)));
    // Elsewhere the system has its own numbered badge.
    #[cfg(not(windows))]
    let _ = window.set_badge_count(text.map(|_| i64::from(count)));
}

/// A bold system font for the digits, if one can be found. Without one the
/// badge is a plain red dot.
#[cfg_attr(not(windows), allow(dead_code))]
fn font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let fonts = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()) + r"\Fonts\";
        [
            format!("{fonts}segoeuib.ttf"),
            format!("{fonts}arialbd.ttf"),
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into(),
        ]
        .iter()
        .find_map(|path| FontVec::try_from_vec(std::fs::read(path).ok()?).ok())
    })
    .as_ref()
}

/// The badge icon: `text` in white on a red circle.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn draw(text: &str) -> Image<'static> {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    let centre = SIZE as f32 / 2.0;
    // Edges smoothed by sampling each pixel 4 by 4 times.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut inside = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let dx = x as f32 + (sx as f32 + 0.5) / 4.0 - centre;
                    let dy = y as f32 + (sy as f32 + 0.5) / 4.0 - centre;
                    if dx * dx + dy * dy <= centre * centre {
                        inside += 1;
                    }
                }
            }
            let i = ((y * SIZE + x) * 4) as usize;
            pixels[i..i + 3].copy_from_slice(&RED);
            pixels[i + 3] = (inside * 255 / 16) as u8;
        }
    }
    if let Some(font) = font() {
        write(&mut pixels, font, text);
    }
    Image::new_owned(pixels, SIZE, SIZE)
}

/// Blends `text` in white into the middle of the circle.
#[cfg_attr(not(windows), allow(dead_code))]
fn write(pixels: &mut [u8], font: &FontVec, text: &str) {
    let width_of = |scale: PxScale| -> f32 {
        let scaled = font.as_scaled(scale);
        text.chars()
            .map(|c| scaled.h_advance(font.glyph_id(c)))
            .sum()
    };
    // As big as fits: one digit large, 99+ smaller to stay inside the circle.
    let room = if text.len() > 2 { 29.0 } else { 25.0 };
    let mut scale = PxScale::from(23.0);
    let width = width_of(scale);
    if width > room {
        scale = PxScale::from(23.0 * room / width);
    }
    let scaled = font.as_scaled(scale);
    let mut x = (SIZE as f32 - width_of(scale)) / 2.0;
    let glyphs: Vec<_> = text
        .chars()
        .filter_map(|c| {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(scale, point(x, scaled.ascent()));
            x += scaled.h_advance(id);
            font.outline_glyph(glyph)
        })
        .collect();
    // Centred on the inked part, so digits sit in the middle whatever the
    // font's line spacing.
    let (top, bottom) = glyphs.iter().fold((f32::MAX, f32::MIN), |(t, b), g| {
        let bounds = g.px_bounds();
        (t.min(bounds.min.y), b.max(bounds.max.y))
    });
    let shift = (SIZE as f32 - (bottom - top)) / 2.0 - top;
    for glyph in &glyphs {
        let bounds = glyph.px_bounds();
        glyph.draw(|gx, gy, coverage| {
            let px = bounds.min.x as i32 + gx as i32;
            let py = (bounds.min.y + shift).round() as i32 + gy as i32;
            if !(0..SIZE as i32).contains(&px) || !(0..SIZE as i32).contains(&py) {
                return;
            }
            let i = ((py as u32 * SIZE + px as u32) * 4) as usize;
            let a = coverage.clamp(0.0, 1.0);
            for c in 0..3 {
                pixels[i + c] = (pixels[i + c] as f32 * (1.0 - a) + 255.0 * a).round() as u8;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_past_two_digits_read_99_plus() {
        assert_eq!(label(0), None);
        assert_eq!(label(1).as_deref(), Some("1"));
        assert_eq!(label(99).as_deref(), Some("99"));
        assert_eq!(label(100).as_deref(), Some("99+"));
        assert_eq!(label(12_345).as_deref(), Some("99+"));
    }

    #[test]
    fn draws_a_round_red_icon() {
        let icon = draw("7");
        assert_eq!((icon.width(), icon.height()), (SIZE, SIZE));
        let alpha = |x: u32, y: u32| icon.rgba()[((y * SIZE + x) * 4 + 3) as usize];
        // Corners are clear, the edge of the circle is solid.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(SIZE - 1, SIZE - 1), 0);
        assert_eq!(alpha(SIZE / 2, 1), 255);
        if let Ok(dir) = std::env::var("NOVERO_BADGE_DUMP") {
            for text in ["3", "42", "99+"] {
                std::fs::write(format!("{dir}/{}.rgba", text.len()), draw(text).rgba()).unwrap();
            }
        }
    }
}
