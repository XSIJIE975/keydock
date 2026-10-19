//! Pure radial-menu geometry: overlay placement and 8-sector hit testing.

use crate::bounds::{work_area_for, Rect};

pub const SECTOR_COUNT: usize = 8;
pub const CENTER_CANCEL_RADIUS: f64 = 36.0;

/// Place a square overlay centered on the cursor, clamped into the work area of
/// the monitor containing the cursor (or the nearest monitor).
pub fn place_overlay(cursor: (i32, i32), size: i32, works: &[Rect]) -> (i32, i32) {
    let work = work_area_for(cursor, works).unwrap_or(Rect::new(0, 0, size, size));
    let half = size / 2;
    let x = crate::bounds::clamp(cursor.0 - half, work.x, work.right() - size);
    let y = crate::bounds::clamp(cursor.1 - half, work.y, work.bottom() - size);
    (x, y)
}

/// Eight 45-degree sectors. Sector 0 starts east and advances clockwise.
/// The center circle is a deliberate cancel zone.
pub fn hit_test(dx: i32, dy: i32) -> Option<usize> {
    let radius_squared = f64::from(dx) * f64::from(dx) + f64::from(dy) * f64::from(dy);
    if radius_squared <= CENTER_CANCEL_RADIUS * CENTER_CANCEL_RADIUS {
        return None;
    }
    let mut degrees = (dy as f64).atan2(dx as f64).to_degrees();
    if degrees < 0.0 {
        degrees += 360.0;
    }
    Some((((degrees + 22.5) / 45.0).floor() as usize) % SECTOR_COUNT)
}

/// Scale values used by the overlay animation: 120ms grow + fade-in.
pub fn entry_scale(elapsed_ms: u32) -> f64 {
    let t = (elapsed_ms.min(120) as f64) / 120.0;
    0.86 + 0.14 * t
}

pub fn entry_opacity(elapsed_ms: u32) -> f64 {
    (elapsed_ms.min(120) as f64) / 120.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect::new(0, 0, 2560, 1440);
    const LEFT: Rect = Rect::new(-1920, 0, 1920, 900);

    #[test]
    fn overlay_is_centered_on_cursor() {
        let (x, y) = place_overlay((1000, 700), 360, &[PRIMARY]);
        assert_eq!((x, y), (820, 520));
    }

    #[test]
    fn overlay_pushed_back_at_edges() {
        let (x, y) = place_overlay((2550, 1430), 360, &[PRIMARY]);
        assert_eq!((x, y), (2200, 1080));
        assert!(x + 360 <= PRIMARY.right());
        assert!(y + 360 <= PRIMARY.bottom());
    }

    #[test]
    fn overlay_supports_negative_monitor_coordinates() {
        let (x, y) = place_overlay((-1900, 100), 360, &[PRIMARY, LEFT]);
        assert_eq!((x, y), (-1920, 0));
    }

    #[test]
    fn overlay_chooses_nearest_monitor_when_outside_all_work_areas() {
        let (x, _) = place_overlay((-2100, 100), 360, &[PRIMARY, LEFT]);
        assert_eq!(x, -1920);
    }

    #[test]
    fn hit_test_covers_all_eight_sectors_and_center() {
        assert_eq!(hit_test(120, 0), Some(0));
        assert_eq!(hit_test(80, 80), Some(1));
        assert_eq!(hit_test(0, 120), Some(2));
        assert_eq!(hit_test(-80, 80), Some(3));
        assert_eq!(hit_test(-120, 0), Some(4));
        assert_eq!(hit_test(-80, -80), Some(5));
        assert_eq!(hit_test(0, -120), Some(6));
        assert_eq!(hit_test(80, -80), Some(7));
        assert_eq!(hit_test(0, 0), None);
    }

    #[test]
    fn entry_animation_is_bounded() {
        assert!((entry_scale(0) - 0.86).abs() < f64::EPSILON);
        assert!((entry_scale(120) - 1.0).abs() < f64::EPSILON);
        assert_eq!(entry_scale(500), 1.0);
        assert_eq!(entry_opacity(0), 0.0);
        assert_eq!(entry_opacity(120), 1.0);
    }
}
