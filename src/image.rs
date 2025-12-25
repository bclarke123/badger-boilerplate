use crate::state::CURRENT_IMAGE;
use core::sync::atomic::Ordering;

static FERRIS_IMG: &[u8] = include_bytes!("../images/julian.bmp");
static REPO_IMG: &[u8] = include_bytes!("../images/repo.bmp");
static MTRAS_LOGO: &[u8] = include_bytes!("../images/mtras_logo.bmp");

static IMAGES: [&[u8]; 3] = [FERRIS_IMG, REPO_IMG, MTRAS_LOGO];
static IMAGE_LOCATIONS: [(i32, i32); 3] = [(0, 24), (190, 26), (190, 26)];

pub fn get_image() -> &'static [u8] {
    IMAGES[CURRENT_IMAGE.load(Ordering::Relaxed)]
}

pub fn get_position() -> (i32, i32) {
    IMAGE_LOCATIONS[CURRENT_IMAGE.load(Ordering::Relaxed)]
}

pub fn next() {
    let current_image = CURRENT_IMAGE.load(Ordering::Relaxed);
    let next = (current_image + 1) % IMAGES.len();
    CURRENT_IMAGE.store(next, Ordering::Relaxed);
}
