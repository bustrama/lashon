//! Injection smoke test — verifies the clipboard survives a text injection.
//!
//! Marked `#[ignore]`: it synthesizes a real Ctrl+V (which lands in whatever
//! window is focused) and uses the system clipboard. Run explicitly:
//!
//! ```text
//! cargo test -p lashon-core --test inject -- --ignored
//! ```

use lashon_core::inject::inject_text;

#[test]
#[ignore = "synthesizes real keyboard input and uses the system clipboard"]
fn injection_preserves_the_clipboard() {
    let sentinel = "lashon-clipboard-sentinel-מקור";
    arboard::Clipboard::new()
        .expect("open clipboard")
        .set_text(sentinel)
        .expect("seed the clipboard");

    inject_text("שלום, זאת בדיקת הזרקה").expect("inject Hebrew text");

    let restored = arboard::Clipboard::new()
        .expect("reopen clipboard")
        .get_text()
        .expect("read clipboard");
    assert_eq!(
        restored, sentinel,
        "clipboard was not restored after injection"
    );
}

#[test]
#[ignore = "synthesizes real keyboard input and uses the system clipboard"]
fn injection_preserves_a_non_text_clipboard() {
    // Seed the clipboard with a 2x2 image — content `get_text` cannot read.
    let image = arboard::ImageData {
        width: 2,
        height: 2,
        bytes: vec![0xFFu8; 2 * 2 * 4].into(),
    };
    arboard::Clipboard::new()
        .expect("open clipboard")
        .set_image(image)
        .expect("seed the clipboard with an image");

    inject_text("שלום, זאת בדיקת הזרקה").expect("inject Hebrew text");

    let restored = arboard::Clipboard::new()
        .expect("reopen clipboard")
        .get_image()
        .expect("clipboard should still hold an image, not the transcript");
    assert_eq!((restored.width, restored.height), (2, 2));
}
