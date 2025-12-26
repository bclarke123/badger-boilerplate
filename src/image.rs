use crate::state::CURRENT_IMAGE;
use core::sync::atomic::Ordering;

static IMAGES: [&[u8]; 3] = [
    include_bytes!("../images/julian.bmp"),
    include_bytes!("../images/tropical.bmp"),
    include_bytes!("../images/2026.bmp"),
];

pub fn get_image() -> &'static [u8] {
    IMAGES[CURRENT_IMAGE.load(Ordering::Relaxed)]
}

pub fn get_position() -> (i32, i32) {
    (0, 24)
}

pub fn next() {
    let current_image = CURRENT_IMAGE.load(Ordering::Relaxed);
    let next = (current_image + 1) % IMAGES.len();
    CURRENT_IMAGE.store(next, Ordering::Relaxed);
}

pub fn prev() {
    let current_image = CURRENT_IMAGE.load(Ordering::Relaxed);
    let prev = (if current_image == 0 {
        IMAGES.len()
    } else {
        current_image
    }) - 1;
    CURRENT_IMAGE.store(prev, Ordering::Relaxed);
}
