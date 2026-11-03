//! `Stamp` header of the legacy SPA API: base64 of the app's CFB constant
//! XOR `"<app id>:<unix seconds>"`, truncated to the shorter of the two
//! (upstream `ApiImplType1._get_stamp`).

use super::AppConstants;

pub fn stamp(constants: &AppConstants, unix_seconds: u64) -> String {
    let raw = format!("{}:{unix_seconds}", constants.app_id);
    let mixed: Vec<u8> = constants
        .cfb
        .iter()
        .zip(raw.as_bytes())
        .map(|(a, b)| a ^ b)
        .collect();
    crate::base64::encode(&mixed)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;

    #[test]
    fn matches_reference_vectors() {
        let c = fixtures::constants();
        let v = fixtures::json("stamp_vectors.json");
        let cases = v["cases"].as_array().expect("cases");
        assert!(!cases.is_empty());
        for case in cases {
            let now = case["now_s"].as_u64().expect("now_s");
            assert_eq!(stamp(&c, now), fixtures::str(case, "expected"), "now {now}");
        }
    }

    #[test]
    fn changes_every_second() {
        let c = fixtures::constants();
        assert_ne!(stamp(&c, 1_793_740_000), stamp(&c, 1_793_740_001));
    }
}
