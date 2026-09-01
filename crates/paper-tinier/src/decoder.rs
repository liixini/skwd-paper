use crate::ffi;
use crate::ivf::ResidentVideo;
use crate::model::{ColorMatrix, MAX_FRAME_PIXELS, VideoInfo};
use dav1d::{Decoder as Dav1dDecoder, Picture, PixelLayout, PlanarImageComponent, Settings};

pub struct DecodedFrame {
    picture: Picture,
    pub width: i32,
    pub height: i32,
}

pub struct Decoder {
    handle: Dav1dDecoder,
}

impl Decoder {
    pub fn open() -> Result<Self, String> {
        let mut settings = Settings::new();
        settings.set_n_threads(1);
        settings.set_max_frame_delay(1);
        settings.set_apply_grain(false);
        settings.set_frame_size_limit(u32::try_from(MAX_FRAME_PIXELS).unwrap());
        let handle = Dav1dDecoder::with_settings(&settings)
            .map_err(|error| format!("cannot create dav1d decoder: {error}"))?;
        Ok(Self { handle })
    }

    pub fn decode(&mut self, source: &ResidentVideo, index: usize) -> Result<DecodedFrame, String> {
        self.handle
            .send_data(source.packet(index)?, Some(index as i64), None, None)
            .map_err(|error| format!("IVF packet {index} send failed: {error}"))?;
        let picture = self.handle.get_picture().map_err(|error| {
            format!("IVF packet {index} did not produce one immediate frame: {error}")
        })?;
        if picture.pixel_layout() != PixelLayout::I420
            || picture.bit_depth() != 8
            || picture.width() == 0
            || picture.height() == 0
            || picture.stride(PlanarImageComponent::Y) < picture.width()
            || picture.stride(PlanarImageComponent::U) < picture.width().div_ceil(2)
            || picture.stride(PlanarImageComponent::Y) > i32::MAX as u32
            || picture.stride(PlanarImageComponent::U) > i32::MAX as u32
        {
            return Err(format!("IVF packet {index} has unsafe non-I420 output"));
        }
        let width = i32::try_from(picture.width()).map_err(|_| "decoded width exceeds i32")?;
        let height = i32::try_from(picture.height()).map_err(|_| "decoded height exceeds i32")?;
        Ok(DecodedFrame { picture, width, height })
    }

    pub fn convert(
        frame: &DecodedFrame,
        video: VideoInfo,
        destination: &mut [u8],
    ) -> Result<(), String> {
        if u32::try_from(frame.width) != Ok(video.width)
            || u32::try_from(frame.height) != Ok(video.height)
        {
            return Err("mid-stream resolution change rejected".into());
        }
        if destination.len() < video.frame_bytes {
            return Err("destination canvas is too small".into());
        }
        let y = frame.picture.plane(PlanarImageComponent::Y);
        let u = frame.picture.plane(PlanarImageComponent::U);
        let v = frame.picture.plane(PlanarImageComponent::V);
        if y.is_empty() || u.is_empty() || v.is_empty() {
            return Err("decoded I420 planes are empty".into());
        }
        let convert = match video.matrix {
            ColorMatrix::Bt601 => ffi::I420ToARGB,
            ColorMatrix::Bt709 => ffi::H420ToARGB,
        };
        let result = unsafe {
            convert(
                y.as_ptr(),
                i32::try_from(frame.picture.stride(PlanarImageComponent::Y)).unwrap(),
                u.as_ptr(),
                i32::try_from(frame.picture.stride(PlanarImageComponent::U)).unwrap(),
                v.as_ptr(),
                i32::try_from(frame.picture.stride(PlanarImageComponent::V)).unwrap(),
                destination.as_mut_ptr(),
                i32::try_from(video.stride).expect("validated video stride fits i32"),
                frame.width,
                frame.height,
            )
        };
        if result == 0 { Ok(()) } else { Err("libyuv conversion failed".into()) }
    }
}

mod tests;
