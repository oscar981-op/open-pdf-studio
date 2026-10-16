//! Bounded image-first page IPC. The text worker prepares an opaque JPEG paint
//! followed by a validated PWPG text scene; the parent revalidates both parts.
//! This protocol does not parse PDF objects or accept arbitrary paint order.
use crate::{winding_page::PreparedWindingPage, Bitmap};
use std::error::Error;

const HEADER: usize = 32;
const LIMIT: usize = 32 * 1024 * 1024;

pub struct PreparedHybridPage {
    image: Bitmap,
    text: PreparedWindingPage,
    retained: usize,
}

impl PreparedHybridPage {
    pub fn new(image: Bitmap, text: PreparedWindingPage) -> Result<Self, Box<dyn Error>> {
        let [width, height] = text.dimensions();
        let image_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("hybrid image length overflow")?;
        if image.width != width
            || image.height != height
            || image.rgba.len() != image_len
            || image.rgba.chunks_exact(4).any(|pixel| pixel[3] != 255)
        {
            return Err("hybrid image must be opaque and match text dimensions".into());
        }
        let retained = image_len
            .checked_add(text.retained_bytes())
            .ok_or("hybrid scene length overflow")?;
        if retained > LIMIT {
            return Err("hybrid retained byte budget".into());
        }
        Ok(Self {
            image,
            text,
            retained,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let scene = self.text.encode()?;
        let len = HEADER
            .checked_add(self.image.rgba.len())
            .and_then(|n| n.checked_add(scene.len()))
            .ok_or("hybrid IPC length overflow")?;
        if len > LIMIT {
            return Err("hybrid IPC byte budget".into());
        }
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(b"PHYG");
        for n in [
            1_u32,
            self.image.width,
            self.image.height,
            u32::try_from(self.image.rgba.len())?,
            u32::try_from(scene.len())?,
            0,
            0,
        ] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&self.image.rgba);
        out.extend_from_slice(&scene);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() < HEADER || bytes.len() > LIMIT || &bytes[..4] != b"PHYG" {
            return Err("hybrid IPC framing".into());
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let [version, width, height, image_len, scene_len, reserved1, reserved2] = [
            word(4),
            word(8),
            word(12),
            word(16),
            word(20),
            word(24),
            word(28),
        ];
        let expected_image = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("hybrid image length overflow")?;
        let expected_len = HEADER
            .checked_add(image_len as usize)
            .and_then(|n| n.checked_add(scene_len as usize))
            .ok_or("hybrid IPC length overflow")?;
        if version != 1
            || reserved1 != 0
            || reserved2 != 0
            || image_len as usize != expected_image
            || expected_len != bytes.len()
            || scene_len < 24
        {
            return Err("hybrid IPC fields".into());
        }
        let text = PreparedWindingPage::decode(&bytes[HEADER + expected_image..])?;
        Self::new(
            Bitmap {
                width,
                height,
                rgba: bytes[HEADER..HEADER + expected_image].to_vec(),
            },
            text,
        )
    }

    pub fn render(&self) -> Result<Bitmap, Box<dyn Error>> {
        self.text.render_over_opaque(self.image.clone())
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained
    }

    pub fn paint_count(&self) -> usize {
        self.text.paint_count() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ordered_opaque_image_and_text_framing() {
        let text = PreparedWindingPage::new(8, 8).unwrap();
        let image = Bitmap {
            width: 8,
            height: 8,
            rgba: vec![255; 8 * 8 * 4],
        };
        let encoded = PreparedHybridPage::new(image, text)
            .unwrap()
            .encode()
            .unwrap();
        assert_eq!(
            PreparedHybridPage::decode(&encoded)
                .unwrap()
                .render()
                .unwrap()
                .rgba
                .len(),
            256
        );
        for at in [0, 4, 8, 12, 16, 20, 24, 28, 35] {
            let mut bad = encoded.clone();
            bad[at] ^= 1;
            assert!(
                PreparedHybridPage::decode(&bad).is_err(),
                "accepted mutation at {at}"
            );
        }
        assert!(PreparedHybridPage::decode(&encoded[..encoded.len() - 1]).is_err());
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(PreparedHybridPage::decode(&trailing).is_err());
    }
}
