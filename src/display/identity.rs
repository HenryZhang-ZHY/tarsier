//! Who made a monitor, used to pick vendor quirks.

/// EDID vendor and product, e.g. `GSM` (LG) / `0x5BF6`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Three-letter PnP manufacturer id.
    pub manufacturer: String,
    pub product: u16,
}

impl Identity {
    /// Parses a Windows monitor device path such as
    /// `\\?\DISPLAY#GSM5BF6#4&311f1208&0&UID41030#{e6f07b5f-...}`.
    pub fn from_device_path(path: &str) -> Option<Self> {
        let hardware_id = path.split('#').nth(1)?;
        if hardware_id.len() != 7 || !hardware_id.is_ascii() {
            return None;
        }
        let (manufacturer, product) = hardware_id.split_at(3);
        if !manufacturer.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        Some(Identity {
            manufacturer: manufacturer.to_string(),
            product: u16::from_str_radix(product, 16).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_path() {
        let id = Identity::from_device_path(
            r"\\?\DISPLAY#GSM5BF6#4&311f1208&0&UID41030#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}",
        );
        assert_eq!(
            id,
            Some(Identity {
                manufacturer: "GSM".into(),
                product: 0x5BF6
            })
        );
    }

    #[test]
    fn rejects_fallback_ids() {
        assert_eq!(Identity::from_device_path(r"\\.\DISPLAY1#0"), None);
        assert_eq!(Identity::from_device_path(""), None);
        assert_eq!(Identity::from_device_path("a#gsm5bf6#b"), None);
    }
}
