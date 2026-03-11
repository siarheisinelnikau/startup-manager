//! Bounded offscreen capture of the actual UI through its production CPU rasterizer.
use crate::ui::Desktop;
use anyhow::{Result, ensure};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

pub fn run(path: &Path, screen: &str, size: [u32; 2], scale: f32) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "Screenshot output must be an absolute path"
    );
    ensure!(
        (800..=2400).contains(&size[0])
            && (600..=1800).contains(&size[1])
            && (1.0..=2.0).contains(&scale),
        "Unsupported capture dimensions or scale"
    );
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(scale);
    let mut desktop = Desktop::preview(&ctx, screen)?;
    let mut renderer = EguiSoftwareRender::new(ColorFieldOrder::Bgra);
    let width = (size[0] as f32 * scale).round() as usize;
    let height = (size[1] as f32 * scale).round() as usize;
    let mut pixels = vec![[0, 0, 0, 255]; width * height];
    let mut last_shapes = vec![];
    let mut stable = 0;
    let start = Instant::now();
    for frame in 0..12 {
        ensure!(
            start.elapsed() < Duration::from_secs(5),
            "Capture exceeded its readiness deadline"
        );
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0] as f32, size[1] as f32),
            )),
            time: Some(frame as f64 / 60.),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| desktop.update(ctx));
        if output.shapes == last_shapes && output.textures_delta.is_empty() {
            stable += 1;
        } else {
            stable = 0;
        }
        last_shapes = output.shapes.clone();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        renderer.render(
            &mut BufferMutRef::new(&mut pixels, width, height),
            &primitives,
            &output.textures_delta,
            output.pixels_per_point,
        );
        if stable >= 2 {
            let mut rgb = Vec::with_capacity(width * height * 3);
            for pixel in &pixels {
                rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Missing screenshot parent"))?;
            fs::create_dir_all(parent)?;
            let temp = parent.join(format!("capture-{}.tmp", uuid::Uuid::new_v4()));
            let file = fs::File::create(&temp)?;
            let mut encoder = png::Encoder::new(file, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&rgb)?;
            writer.finish()?;
            fs::rename(temp, path)?;
            fs::write(
                path.with_extension("json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"version":crate::VERSION,"backend":"production CPU rasterizer","surface":"client area, offscreen","screen":screen,"scenario":"deterministic isolated fixtures; all actions disabled","physical_pixels":[width,height],"logical_size":size,"scale":scale,"frames":frame+1,"elapsed_ms":start.elapsed().as_millis()}),
                )?,
            )?;
            return Ok(());
        }
    }
    anyhow::bail!("Capture did not reach a stable layout within 12 frames")
}
