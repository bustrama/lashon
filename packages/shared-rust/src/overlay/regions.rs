// Adapted from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: the island rectangle the front end pushes to the cursor poll, in
// windows/src-tauri/src/island.rs, generalised here to several named regions
// with a scale. See THIRD-PARTY-NOTICES.

//! Interactive regions: the parts of the overlay window that take the mouse.
//!
//! The overlay window is much larger than what it draws. Everything outside
//! the creature and its cards must let clicks through to the app underneath,
//! so the frontend reports the rectangles it draws interactive content in, and
//! the cursor poll hit-tests against the last report.
//!
//! Rectangles are in **physical pixels relative to the window's top-left**,
//! measured at the scale factor sent with the report. When the window has
//! since moved to a display with another scale and the frontend has not
//! re-reported yet, [`RegionSet::hit`] rescales the stale report instead of
//! hit-testing pixels from the wrong display.

use serde::{Deserialize, Serialize};

use super::geometry::{Point, Rect};

/// The most regions one report may carry. The overlay draws a handful (the
/// creature, a card, a bubble); a cap keeps a runaway report from turning
/// every 16 ms poll tick into a long scan.
pub const MAX_REGIONS: usize = 32;

/// The longest region id, in bytes.
pub const MAX_ID_LEN: usize = 64;

/// One interactive rectangle, as the frontend reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// What the region is (`creature`, `island`, …). It comes back in hover
    /// events, so the frontend knows what the cursor is over.
    pub id: String,
    #[serde(flatten)]
    pub rect: Rect,
}

/// Why a report was rejected. The previous report stays in force.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum RegionError {
    #[error("{0} regions in one report; at most {MAX_REGIONS} are allowed")]
    TooMany(usize),
    #[error("a region id is empty or longer than {MAX_ID_LEN} bytes")]
    BadId,
    #[error("region `{0}` has a non-finite or negative size")]
    BadRect(String),
    #[error("{0} is not a valid scale factor")]
    BadScale(f64),
    #[error("report epoch {0} was never handed out")]
    UnknownEpoch(u64),
}

/// The last accepted report.
#[derive(Debug, Clone)]
pub struct RegionSet {
    regions: Vec<Region>,
    scale: f64,
    version: u64,
    /// The stamp of the last report applied.
    stamp: Option<Stamp>,
    /// The last epoch handed to a page.
    epochs: u64,
}

impl Default for RegionSet {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            scale: 1.0,
            version: 0,
            stamp: None,
            epochs: 0,
        }
    }
}

/// Where a report sits in the order the frontend sent them.
///
/// Reports arrive through async commands, which the shell may handle out of
/// order. An older report applied after a newer one would leave stale
/// rectangles in force until the next change. `epoch` names the page that
/// sent it, from [`RegionSet::new_epoch`]: a reloaded page gets a higher one,
/// so no report from before the reload can undo one sent since. `seq` counts
/// the reports one page sends.
///
/// Stamps compare by epoch, then by count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    pub epoch: u64,
    pub seq: u64,
}

impl RegionSet {
    /// Start a new epoch for a page about to report, later than every page
    /// before it.
    pub fn new_epoch(&mut self) -> u64 {
        self.epochs += 1;
        self.epochs
    }

    /// Replace the report with one the frontend stamped, unless a report it
    /// sent later has already been applied. Returns whether anything changed.
    ///
    /// A late report is dropped without an error: what replaced it is
    /// already in force. A rejected report leaves the order as it was.
    pub fn replace_in_order(
        &mut self,
        regions: Vec<Region>,
        scale: f64,
        stamp: Stamp,
    ) -> Result<bool, RegionError> {
        // An epoch never handed out would outrank every page after it.
        if stamp.epoch == 0 || stamp.epoch > self.epochs {
            return Err(RegionError::UnknownEpoch(stamp.epoch));
        }
        if self.stamp.is_some_and(|last| stamp <= last) {
            return Ok(false);
        }
        let changed = self.replace(regions, scale)?;
        self.stamp = Some(stamp);
        Ok(changed)
    }

    /// Replace the report. Returns whether anything changed.
    ///
    /// The whole report is validated first; a bad one is rejected and the
    /// previous report stays. Zero-sized regions are dropped, since they can
    /// never be hit.
    pub fn replace(&mut self, regions: Vec<Region>, scale: f64) -> Result<bool, RegionError> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(RegionError::BadScale(scale));
        }
        if regions.len() > MAX_REGIONS {
            return Err(RegionError::TooMany(regions.len()));
        }
        for region in &regions {
            if region.id.is_empty() || region.id.len() > MAX_ID_LEN {
                return Err(RegionError::BadId);
            }
            if !region.rect.is_valid() {
                return Err(RegionError::BadRect(region.id.clone()));
            }
        }
        let regions: Vec<Region> = regions
            .into_iter()
            .filter(|r| r.rect.width > 0.0 && r.rect.height > 0.0)
            .collect();
        if regions == self.regions && scale == self.scale {
            return Ok(false);
        }
        self.regions = regions;
        self.scale = scale;
        self.version = self.version.wrapping_add(1);
        Ok(true)
    }

    /// Bumped on every accepted change, so the poll can tell a new report
    /// from the one it already decided on.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The id of the region under `local`, or `None`.
    ///
    /// - `local` is the cursor in physical pixels relative to the window's
    ///   top-left, at the window's current scale `scale_now`.
    /// - `margin` (physical pixels at `scale_now`) grows every region, so the
    ///   window already takes the mouse when a moving cursor reaches a button.
    /// - When regions overlap, the one reported last wins: the frontend reports
    ///   in document order, and later elements paint on top.
    pub fn hit(&self, local: Point, scale_now: f64, margin: f64) -> Option<&str> {
        let k = if scale_now.is_finite() && scale_now > 0.0 {
            scale_now / self.scale
        } else {
            1.0
        };
        self.regions
            .iter()
            .rev()
            .find(|r| r.rect.scaled(k).inflate(margin).contains(local))
            .map(|r| r.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: &str, x: f64, y: f64, w: f64, h: f64) -> Region {
        Region {
            id: id.to_string(),
            rect: Rect::new(x, y, w, h),
        }
    }

    #[test]
    fn empty_set_hits_nothing() {
        let set = RegionSet::default();
        assert!(set.is_empty());
        assert_eq!(set.hit(Point::new(5.0, 5.0), 1.0, 0.0), None);
    }

    #[test]
    fn hits_the_region_under_the_cursor() {
        let mut set = RegionSet::default();
        set.replace(vec![region("creature", 100.0, 300.0, 96.0, 96.0)], 1.0)
            .unwrap();
        assert_eq!(
            set.hit(Point::new(150.0, 350.0), 1.0, 0.0),
            Some("creature")
        );
        assert_eq!(set.hit(Point::new(50.0, 350.0), 1.0, 0.0), None);
    }

    #[test]
    fn margin_grows_the_target() {
        let mut set = RegionSet::default();
        set.replace(vec![region("island", 100.0, 100.0, 50.0, 20.0)], 1.0)
            .unwrap();
        let near = Point::new(95.0, 110.0);
        assert_eq!(set.hit(near, 1.0, 0.0), None);
        assert_eq!(set.hit(near, 1.0, 8.0), Some("island"));
    }

    #[test]
    fn last_reported_region_wins_an_overlap() {
        let mut set = RegionSet::default();
        set.replace(
            vec![
                region("creature", 0.0, 0.0, 100.0, 100.0),
                region("island", 50.0, 50.0, 100.0, 100.0),
            ],
            1.0,
        )
        .unwrap();
        assert_eq!(set.hit(Point::new(75.0, 75.0), 1.0, 0.0), Some("island"));
        assert_eq!(set.hit(Point::new(25.0, 25.0), 1.0, 0.0), Some("creature"));
    }

    #[test]
    fn a_stale_report_is_rescaled_to_the_current_display() {
        // Reported at 150 %: a 96 px CSS box at (100, 300) CSS px.
        let mut set = RegionSet::default();
        set.replace(vec![region("creature", 150.0, 450.0, 144.0, 144.0)], 1.5)
            .unwrap();
        // The window moved to a 100 % display before the frontend
        // re-reported: the same box is now at (100, 300) physical.
        let inside_now = Point::new(110.0, 310.0);
        assert_eq!(set.hit(inside_now, 1.0, 0.0), Some("creature"));
        // Hit-testing the unscaled report would have missed it.
        assert_eq!(set.hit(inside_now, 1.5, 0.0), None);
    }

    #[test]
    fn identical_report_is_not_a_change() {
        let mut set = RegionSet::default();
        let report = vec![region("creature", 1.0, 2.0, 3.0, 4.0)];
        assert!(set.replace(report.clone(), 1.25).unwrap());
        let v = set.version();
        assert!(!set.replace(report.clone(), 1.25).unwrap());
        assert_eq!(set.version(), v);
        // The same rectangles at another scale are a change.
        assert!(set.replace(report, 1.5).unwrap());
        assert_ne!(set.version(), v);
    }

    #[test]
    fn zero_sized_regions_are_dropped() {
        let mut set = RegionSet::default();
        set.replace(
            vec![
                region("gone", 10.0, 10.0, 0.0, 20.0),
                region("flat", 10.0, 10.0, 20.0, 0.0),
            ],
            1.0,
        )
        .unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn bad_reports_are_rejected_and_the_old_one_stays() {
        let mut set = RegionSet::default();
        set.replace(vec![region("creature", 0.0, 0.0, 10.0, 10.0)], 1.0)
            .unwrap();
        let v = set.version();

        let too_many = (0..=MAX_REGIONS)
            .map(|i| region(&format!("r{i}"), 0.0, 0.0, 1.0, 1.0))
            .collect();
        assert_eq!(
            set.replace(too_many, 1.0),
            Err(RegionError::TooMany(MAX_REGIONS + 1))
        );
        assert_eq!(
            set.replace(vec![region("", 0.0, 0.0, 1.0, 1.0)], 1.0),
            Err(RegionError::BadId)
        );
        let long_id = "x".repeat(MAX_ID_LEN + 1);
        assert_eq!(
            set.replace(vec![region(&long_id, 0.0, 0.0, 1.0, 1.0)], 1.0),
            Err(RegionError::BadId)
        );
        assert_eq!(
            set.replace(vec![region("nan", f64::NAN, 0.0, 1.0, 1.0)], 1.0),
            Err(RegionError::BadRect("nan".into()))
        );
        assert_eq!(
            set.replace(vec![region("neg", 0.0, 0.0, -1.0, 1.0)], 1.0),
            Err(RegionError::BadRect("neg".into()))
        );
        assert_eq!(
            set.replace(Vec::new(), 0.0),
            Err(RegionError::BadScale(0.0))
        );
        assert!(matches!(
            set.replace(Vec::new(), f64::NAN),
            Err(RegionError::BadScale(_))
        ));

        assert_eq!(set.version(), v);
        assert_eq!(set.hit(Point::new(5.0, 5.0), 1.0, 0.0), Some("creature"));
    }

    fn stamp(epoch: u64, seq: u64) -> Stamp {
        Stamp { epoch, seq }
    }

    #[test]
    fn every_page_gets_a_later_epoch() {
        let mut set = RegionSet::default();
        let first = set.new_epoch();
        assert!(first > 0);
        assert!(set.new_epoch() > first);
    }

    #[test]
    fn a_late_report_cannot_replace_a_newer_one() {
        let mut set = RegionSet::default();
        let page = set.new_epoch();
        let newer = vec![region("island", 0.0, 0.0, 10.0, 10.0)];
        let older = vec![region("creature", 50.0, 50.0, 10.0, 10.0)];
        assert!(set.replace_in_order(newer, 1.0, stamp(page, 2)).unwrap());
        // Sent first, handled second: dropped.
        assert!(!set
            .replace_in_order(older.clone(), 1.0, stamp(page, 1))
            .unwrap());
        assert_eq!(set.hit(Point::new(5.0, 5.0), 1.0, 0.0), Some("island"));
        // A repeat of the last report's stamp is not newer either.
        assert!(!set.replace_in_order(older, 1.0, stamp(page, 2)).unwrap());
        assert_eq!(set.hit(Point::new(55.0, 55.0), 1.0, 0.0), None);
    }

    #[test]
    fn a_reloaded_page_counts_from_the_start_again() {
        let mut set = RegionSet::default();
        let before = set.new_epoch();
        set.replace_in_order(
            vec![region("creature", 0.0, 0.0, 10.0, 10.0)],
            1.0,
            stamp(before, 500),
        )
        .unwrap();
        let reloaded = set.new_epoch();
        let fresh = vec![region("island", 0.0, 0.0, 10.0, 10.0)];
        assert!(set
            .replace_in_order(fresh, 1.0, stamp(reloaded, 1))
            .unwrap());
        assert_eq!(set.hit(Point::new(5.0, 5.0), 1.0, 0.0), Some("island"));
    }

    #[test]
    fn a_report_from_before_a_reload_cannot_undo_the_reloaded_page() {
        let mut set = RegionSet::default();
        let before = set.new_epoch();
        let reloaded = set.new_epoch();
        let fresh = vec![region("island", 0.0, 0.0, 10.0, 10.0)];
        assert!(set
            .replace_in_order(fresh, 1.0, stamp(reloaded, 1))
            .unwrap());
        // Sent by the old page before the reload, handled after the new
        // page's first report: dropped, however far its count got.
        let stale = vec![region("creature", 50.0, 50.0, 10.0, 10.0)];
        assert!(!set
            .replace_in_order(stale, 1.0, stamp(before, 501))
            .unwrap());
        assert_eq!(set.hit(Point::new(5.0, 5.0), 1.0, 0.0), Some("island"));
        assert_eq!(set.hit(Point::new(55.0, 55.0), 1.0, 0.0), None);
    }

    #[test]
    fn rejects_an_epoch_never_handed_out() {
        let mut set = RegionSet::default();
        let report = vec![region("creature", 0.0, 0.0, 10.0, 10.0)];
        assert_eq!(
            set.replace_in_order(report.clone(), 1.0, stamp(0, 1)),
            Err(RegionError::UnknownEpoch(0))
        );
        let page = set.new_epoch();
        assert_eq!(
            set.replace_in_order(report.clone(), 1.0, stamp(page + 1, 1)),
            Err(RegionError::UnknownEpoch(page + 1))
        );
        assert!(set.is_empty());
        // Nothing was taken as applied: the page's first report still is.
        assert!(set.replace_in_order(report, 1.0, stamp(page, 1)).unwrap());
    }

    #[test]
    fn an_unchanged_report_still_moves_the_order_on() {
        let mut set = RegionSet::default();
        let page = set.new_epoch();
        let report = vec![region("creature", 0.0, 0.0, 10.0, 10.0)];
        set.replace_in_order(report.clone(), 1.0, stamp(page, 1))
            .unwrap();
        assert!(!set.replace_in_order(report, 1.0, stamp(page, 3)).unwrap());
        // Older than the unchanged report, so older than what is in force.
        assert!(!set
            .replace_in_order(Vec::new(), 1.0, stamp(page, 2))
            .unwrap());
        assert!(!set.is_empty());
    }

    #[test]
    fn a_rejected_report_leaves_the_order_alone() {
        let mut set = RegionSet::default();
        let page = set.new_epoch();
        set.replace_in_order(
            vec![region("creature", 0.0, 0.0, 10.0, 10.0)],
            1.0,
            stamp(page, 1),
        )
        .unwrap();
        assert_eq!(
            set.replace_in_order(Vec::new(), 0.0, stamp(page, 3)),
            Err(RegionError::BadScale(0.0))
        );
        // Report 2 was sent after the last one applied, so it applies.
        assert!(set
            .replace_in_order(Vec::new(), 1.0, stamp(page, 2))
            .unwrap());
        assert!(set.is_empty());
    }

    #[test]
    fn deserializes_the_frontend_shape() {
        let json = r#"[{"id":"creature","x":10.5,"y":20,"width":96,"height":96}]"#;
        let regions: Vec<Region> = serde_json::from_str(json).unwrap();
        assert_eq!(regions, vec![region("creature", 10.5, 20.0, 96.0, 96.0)]);
    }
}
