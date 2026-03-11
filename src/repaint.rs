//! Scheduling is independent of windowing so idle/exposure behavior is testable.
use std::time::Instant;

#[derive(Default)]
pub struct Repaint {
    pub dirty: bool,
    pub queued: bool,
    pub deadline: Option<Instant>,
}
impl Repaint {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn request_at(&mut self, when: Instant) {
        self.deadline = Some(self.deadline.map_or(when, |old| old.min(when)));
    }
    pub fn needs_ui(&self, now: Instant) -> bool {
        self.dirty || self.deadline.is_some_and(|deadline| deadline <= now)
    }
    pub fn queue(&mut self, now: Instant, visible: bool) -> bool {
        if visible && !self.queued && self.needs_ui(now) {
            self.queued = true;
            true
        } else {
            false
        }
    }
    pub fn begin_frame(&mut self, now: Instant) {
        self.queued = false;
        self.dirty = false;
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
        }
    }
}

#[derive(Default)]
pub struct SceneCache {
    shapes: Vec<egui::epaint::ClippedShape>,
    size: [u32; 2],
    scale: f32,
}
impl SceneCache {
    pub fn changed(
        &self,
        shapes: &[egui::epaint::ClippedShape],
        size: [u32; 2],
        scale: f32,
        textures_changed: bool,
    ) -> bool {
        textures_changed || size != self.size || scale != self.scale || shapes != self.shapes
    }
    pub fn store(&mut self, shapes: Vec<egui::epaint::ClippedShape>, size: [u32; 2], scale: f32) {
        self.shapes = shapes;
        self.size = size;
        self.scale = scale;
    }
    pub fn shapes(&self) -> &[egui::epaint::ClippedShape] {
        &self.shapes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn consumed_redraw_stays_idle_and_hidden_work_waits_for_restore() {
        let now = Instant::now();
        let mut repaint = Repaint::default();
        repaint.invalidate();
        assert!(!repaint.queue(now, false));
        assert!(repaint.queue(now, true));
        assert!(!repaint.queue(now, true));
        repaint.begin_frame(now);
        for _ in 0..100 {
            assert!(!repaint.queue(now, true));
        }
        repaint.invalidate();
        assert!(repaint.queue(now, true));
    }
    #[test]
    fn animation_deadline_survives_early_input_and_stops_after_consumption() {
        let now = Instant::now();
        let future = now + Duration::from_millis(100);
        let mut repaint = Repaint::default();
        repaint.request_at(future);
        repaint.invalidate();
        repaint.begin_frame(now);
        assert_eq!(repaint.deadline, Some(future));
        assert!(!repaint.queue(now, true));
        assert!(repaint.queue(future, true));
        repaint.begin_frame(future);
        assert_eq!(repaint.deadline, None);
        assert!(!repaint.queue(future, true));
    }
    #[test]
    fn equal_scene_skips_raster_but_resize_scale_and_texture_changes_do_not() {
        let mut cache = SceneCache::default();
        let shapes = vec![];
        let size = [900, 600];
        assert!(cache.changed(&shapes, size, 1., false));
        cache.store(shapes, size, 1.);
        assert!(!cache.changed(&[], size, 1., false));
        assert!(cache.changed(&[], [901, 600], 1., false));
        assert!(cache.changed(&[], size, 1.5, false));
        assert!(cache.changed(&[], size, 1., true));
    }
}
