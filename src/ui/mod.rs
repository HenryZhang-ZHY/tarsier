pub mod break_overlay;
pub mod main_window;
mod number_field;

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::*;

icon_assets!(
    ExtraIcons,
    [
        Activity,
        ArrowLeftRight,
        Bug,
        ChartColumn,
        Coffee,
        Contrast,
        Flame,
        Laptop,
        Monitor,
        Power,
        RefreshCw,
        Sun,
        Timer,
        Trophy,
    ]
);

/// The component library's default icons plus the few extra Lucide icons we use.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(data) => Ok(Some(data)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = ExtraIcons.list(path)?;
        all.extend(Assets.list(path)?);
        Ok(all)
    }
}

pub fn format_minutes(secs: u64) -> String {
    let mins = secs / 60;
    if mins >= 60 {
        format!("{} 小时 {} 分钟", mins / 60, mins % 60)
    } else {
        format!("{mins} 分钟")
    }
}

pub fn format_clock(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
