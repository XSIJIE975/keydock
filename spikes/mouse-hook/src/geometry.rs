//! Pure geometry used by the radial overlay. Kept free of Win32 so it can be
//! unit-tested without a GUI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WorkRect {
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    fn distance_squared(self, x: i32, y: i32) -> i64 {
        let dx = if x < self.left {
            self.left - x
        } else if x >= self.right {
            x - self.right + 1
        } else {
            0
        };
        let dy = if y < self.top {
            self.top - y
        } else if y >= self.bottom {
            y - self.bottom + 1
        } else {
            0
        };
        i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
    }
}

/// Place a square overlay of `size` centered on `cursor`, clamped into the
/// work area of the monitor containing the cursor (or the nearest monitor).
pub fn place_overlay(cursor: (i32, i32), size: i32, works: &[WorkRect]) -> (i32, i32) {
    let work = works
        .iter()
        .find(|work| work.contains(cursor.0, cursor.1))
        .copied()
        .or_else(|| {
            works
                .iter()
                .copied()
                .min_by_key(|work| work.distance_squared(cursor.0, cursor.1))
        })
        .unwrap_or(WorkRect {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        });

    let left = if work.width() <= size {
        work.left
    } else {
        (cursor.0 - size / 2).clamp(work.left, work.right - size)
    };
    let top = if work.height() <= size {
        work.top
    } else {
        (cursor.1 - size / 2).clamp(work.top, work.bottom - size)
    };
    (left, top)
}

/// Eight 45-degree sectors. Sector 0 starts east and advances clockwise.
/// The center circle (radius <= 36 px) is a deliberate cancel zone.
pub fn hit_test(dx: i32, dy: i32) -> Option<usize> {
    let radius_squared = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
    if radius_squared <= 36 * 36 {
        return None;
    }
    let mut degrees = (dy as f64).atan2(dx as f64).to_degrees();
    if degrees < 0.0 {
        degrees += 360.0;
    }
    let sector = (((degrees + 22.5) / 45.0).floor() as usize) % 8;
    Some(sector)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: WorkRect = WorkRect {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    const LEFT_NEGATIVE: WorkRect = WorkRect {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 900,
    };

    #[test]
    fn places_centered_in_the_middle() {
        let (x, y) = place_overlay((1000, 700), 360, &[PRIMARY]);
        assert_eq!((x, y), (820, 520));
    }

    #[test]
    fn flips_inside_the_right_and_bottom_edges() {
        let (x, y) = place_overlay((2550, 1430), 360, &[PRIMARY]);
        assert_eq!((x, y), (2200, 1080));
        assert!(x + 360 <= PRIMARY.right);
        assert!(y + 360 <= PRIMARY.bottom);
    }

    #[test]
    fn supports_negative_monitor_coordinates() {
        let (x, y) = place_overlay((-1900, 100), 360, &[PRIMARY, LEFT_NEGATIVE]);
        assert_eq!(x, -1920);
        assert_eq!(y, 0);
        assert!(x >= LEFT_NEGATIVE.left && x + 360 <= LEFT_NEGATIVE.right);
    }

    #[test]
    fn chooses_nearest_monitor_when_cursor_is_outside_all_work_areas() {
        let (x, _) = place_overlay((-2100, 100), 360, &[PRIMARY, LEFT_NEGATIVE]);
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
        assert_eq!(hit_test(20, 20), None);
    }
}
