//! Geographic basics.

/// Mean Earth radius in metres (IUGG).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    pub fn is_valid(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }

    /// Great-circle distance in metres (haversine).
    pub fn distance_m(&self, other: &LatLon) -> f64 {
        let (p1, p2) = (self.lat.to_radians(), other.lat.to_radians());
        let dp = p2 - p1;
        let dl = (other.lon - self.lon).to_radians();
        let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_M * a.sqrt().asin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paris_to_lyon_is_about_392_km() {
        let paris = LatLon::new(48.8566, 2.3522);
        let lyon = LatLon::new(45.7640, 4.8357);
        let km = paris.distance_m(&lyon) / 1000.0;
        assert!((390.0..395.0).contains(&km), "got {km}");
    }

    #[test]
    fn zero_distance_and_validity() {
        let p = LatLon::new(43.3, 5.4);
        assert_eq!(p.distance_m(&p), 0.0);
        assert!(p.is_valid());
        assert!(!LatLon::new(91.0, 0.0).is_valid());
        assert!(!LatLon::new(f64::NAN, 0.0).is_valid());
    }
}
