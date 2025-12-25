use embedded_graphics::prelude::Point;

use crate::state::CURRENT_IMAGE;

static NUMBER_OF_IMAGES: u8 = 3;
static FERRIS_IMG: &[u8] = include_bytes!("../images/julian.bmp");
static REPO_IMG: &[u8] = include_bytes!("../images/repo.bmp");
static MTRAS_LOGO: &[u8] = include_bytes!("../images/mtras_logo.bmp");

pub enum DisplayImage {
    Ferris = 0,
    Repo = 1,
    MtrasLogo = 2,
}

pub fn get_current_image() -> DisplayImage {
    DisplayImage::from_u8(CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed)).unwrap()
}

pub fn next() {
    let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
    let new_image = DisplayImage::from_u8(current_image).unwrap().next();
    CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
}

impl DisplayImage {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ferris),
            1 => Some(Self::Repo),
            2 => Some(Self::MtrasLogo),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            Self::Ferris => 0,
            Self::Repo => 1,
            Self::MtrasLogo => 2,
        }
    }

    pub fn image(&self) -> &'static [u8] {
        match self {
            Self::Ferris => FERRIS_IMG,
            Self::Repo => REPO_IMG,
            Self::MtrasLogo => MTRAS_LOGO,
        }
    }

    pub fn next(&self) -> Self {
        let image_count = self.as_u8();
        let next_image = (image_count + 1) % NUMBER_OF_IMAGES;
        DisplayImage::from_u8(next_image).unwrap()
    }

    // pub fn previous(&self) -> Self {
    //     let image_count = self.as_u8();
    //     if image_count == 0 {
    //         return DisplayImage::from_u8(NUMBER_OF_IMAGES - 1).unwrap();
    //     }
    //     let previous_image = (image_count - 1) % NUMBER_OF_IMAGES;
    //     DisplayImage::from_u8(previous_image).unwrap()
    // }

    pub fn image_location(&self) -> Point {
        match self {
            Self::Ferris => Point::new(0, 24),
            Self::Repo => Point::new(190, 26),
            Self::MtrasLogo => Point::new(190, 26),
        }
    }
}
