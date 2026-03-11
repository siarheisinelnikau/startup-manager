//! Application identity shared by the resource compiler and window host.
pub const SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 256];
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let px = (x as f32 + 0.5) * 32. / size as f32;
            let py = (y as f32 + 0.5) * 32. / size as f32;
            let color = if (5.0..27.0).contains(&px) && (5.0..27.0).contains(&py) {
                if (10.0..22.0).contains(&px) && (14.0..18.0).contains(&py) {
                    [255, 235, 195, 255]
                } else if px >= 23. && py >= 23. {
                    [229, 148, 66, 255]
                } else {
                    [13, 103, 98, 255]
                }
            } else {
                [0, 0, 0, 0]
            };
            let offset = ((y * size + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
    pixels
}

#[allow(dead_code)] // Also compiled by build.rs.
pub fn ico() -> Vec<u8> {
    let mut output = vec![0, 0, 1, 0];
    output.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    let mut images = Vec::new();
    for size in SIZES {
        let rgba = rgba(size);
        let stride = size.div_ceil(32) * 4;
        let mut image = Vec::new();
        for value in [40u32, size, size * 2] {
            image.extend_from_slice(&value.to_le_bytes());
        }
        image.extend_from_slice(&1u16.to_le_bytes());
        image.extend_from_slice(&32u16.to_le_bytes());
        for value in [0, size * size * 4 + stride * size, 0, 0, 0, 0] {
            image.extend_from_slice(&value.to_le_bytes());
        }
        for y in (0..size).rev() {
            for x in 0..size {
                let i = ((y * size + x) * 4) as usize;
                image.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        for y in (0..size).rev() {
            let mut mask = vec![0u8; stride as usize];
            for x in 0..size {
                if rgba[((y * size + x) * 4 + 3) as usize] == 0 {
                    mask[(x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
            image.extend(mask);
        }
        let side = if size == 256 { 0 } else { size as u8 };
        output.extend_from_slice(&[side, side, 0, 0]);
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&32u16.to_le_bytes());
        output.extend_from_slice(&(image.len() as u32).to_le_bytes());
        output.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
        images.push(image);
    }
    for image in images {
        output.extend(image);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_has_visible_artwork_and_transparent_padding_at_every_native_size() {
        for size in SIZES {
            let image = rgba(size);
            let visible = image.chunks_exact(4).filter(|p| p[3] == 255).count();
            assert!(visible > (size * size / 3) as usize);
            assert_eq!(image[3], 0);
            assert!(image.chunks_exact(4).any(|p| p == [255, 235, 195, 255]));
        }
        assert_eq!(&ico()[..6], &[0, 0, 1, 0, 7, 0]);
    }
}
