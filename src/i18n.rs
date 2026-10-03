//! The language the UI is drawn in.
//!
//! English is the source language. Every user-facing string is written in
//! English in the code and wrapped in [`tr!`], which looks the text up in the
//! tables below when the user has picked Chinese. A string with no entry falls
//! back to its English source, so a forgotten translation shows up as English
//! rather than as an empty label or a raw key — and the debug log names it.
//!
//! Adding a string: write it in English, wrap it in [`tr!`], and add the pair
//! to [`ZH`]. The main window's own copy lives in [`MAIN_WINDOW`], which is the
//! same kind of table kept apart only because it is the largest surface by far.
//!
//! Names the user typed (monitor input names, computer names) and what the
//! hardware reports ("DisplayPort 2", "HDMI 1") are data, not copy, and are
//! never translated.
//!
//! Strings the component library owns are not in these tables either. It keeps
//! its own translations — English, Simplified and Traditional Chinese, Italian
//! — in `rust-i18n` locale files and reads them through its own `set_locale`,
//! which [`set_language`] drives, so a single switch moves the whole window.
//! Those are the strings we cannot write ourselves: the cut/copy/paste menu a
//! text field opens, for instance.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// Which language the UI is drawn in. English unless the user says otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Zh,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::En, Self::Zh];

    /// The language's own name, in its own language, and deliberately not
    /// translated: whoever picked the wrong one still has to be able to find
    /// their way back out.
    pub fn label(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Zh => "中文",
        }
    }

    /// What this language is called in the component library's locale files.
    /// It has a vocabulary of its own — the Chinese one is "zh-CN", not "zh" —
    /// and a key that names no file falls back to English in silence.
    pub fn locale(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh-CN",
        }
    }
}

/// The language every window draws in. Written once from the saved config at
/// startup and again whenever the user changes it; only ever read on the UI
/// thread. Tests leave it alone — they assert the English default, or ask for
/// a language explicitly through [`tr_in`].
static CURRENT: AtomicU8 = AtomicU8::new(Language::En as u8);

/// Switches the whole program over, wherever the strings come from.
///
/// Ours are looked up at draw time, so nothing has to be told about the change
/// beyond a repaint. The component library keeps its own strings in `rust-i18n`
/// locale files and has to be told separately; every caller comes through here,
/// which is what keeps the two from drifting apart.
pub fn set_language(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
    gpui_kit::component::set_locale(language.locale());
}

pub fn language() -> Language {
    if CURRENT.load(Ordering::Relaxed) == Language::Zh as u8 {
        Language::Zh
    } else {
        Language::En
    }
}

/// `source` in `language`. English is the source, so it needs no table.
pub fn tr_in(language: Language, source: &'static str) -> &'static str {
    match language {
        Language::En => source,
        Language::Zh => zh_table().get(source).copied().unwrap_or_else(|| {
            log::debug!("no Chinese translation for {source:?}");
            source
        }),
    }
}

/// `source` in the language the user picked. Used for a string that is already
/// a `&'static str` rather than a literal, which [`tr!`] cannot take.
pub fn translate(source: &'static str) -> &'static str {
    tr_in(language(), source)
}

/// `source` with every `{name}` placeholder replaced. Substitution happens
/// after the lookup, so the table holds `{name}` too and word order is free to
/// differ between languages.
pub fn tr_args_in(language: Language, source: &'static str, args: &[(&str, String)]) -> String {
    let mut text = tr_in(language, source).to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

pub fn tr_args(source: &'static str, args: &[(&str, String)]) -> String {
    tr_args_in(language(), source, args)
}

/// The chosen form of a counted string, with `{n}` and any `extra`
/// placeholders filled. Chinese has no plural, so both keys carry the same
/// translation there; English needs the pair to agree with the number.
pub fn tr_args_count(source: &'static str, n: String, extra: &[(&str, String)]) -> String {
    let mut args = Vec::with_capacity(extra.len() + 1);
    args.push(("n", n));
    args.extend(extra.iter().cloned());
    tr_args(source, &args)
}

/// The English source text, in the UI's current language.
///
/// ```ignore
/// tr!("Quit")
/// tr!("Switched to {label}", label = name)
/// tr!(n = minutes, "Break in 1 minute" | "Break in {n} minutes")
/// tr!(n = worked, "Worked for 1 minute. {tip}" | "Worked for {n} minutes. {tip}", tip = self.tip)
/// ```
///
/// A placeholder is named after the argument that fills it, except in the
/// counted form, where the count is always `{n}` whatever the variable is
/// called. The source has to be a literal; for a string that is already a
/// `&'static str` — a table of labels, say — call [`translate`] instead.
///
/// The arms stop at a comma rather than accepting a trailing one: an optional
/// trailing `$(,)?` after a comma-separated `expr` repetition swallows the
/// separator and ends the repetition after the first pair.
macro_rules! tr {
    (n = $n:expr, $one:literal | $many:literal $(, $name:ident = $value:expr)*) => {{
        let n = $n;
        let source = if n == 1 { $one } else { $many };
        let extra: Vec<(&str, String)> = vec![$( (stringify!($name), ($value).to_string()) ),*];
        $crate::i18n::tr_args_count(source, n.to_string(), &extra)
    }};
    ($source:literal $(, $name:ident = $value:expr)+) => {{
        let args: Vec<(&str, String)> = vec![$( (stringify!($name), ($value).to_string()) ),+];
        $crate::i18n::tr_args($source, &args)
    }};
    ($source:literal) => {
        $crate::i18n::translate($source)
    };
}

pub(crate) use tr;

/// English source text → 简体中文, for everything outside the main window.
static ZH: &[(&str, &str)] = &[
    // ---- durations and preferences ------------------------------------
    ("{n} h {m} min", "{n} 小时 {m} 分钟"),
    ("{n} min", "{n} 分钟"),
    ("; ", "；"),
    ("Light", "亮色"),
    ("Dark", "暗色"),
    ("System", "跟随系统"),
    // ---- the name this computer gives itself --------------------------
    ("This PC", "本机"),
    // ---- tray menu ----------------------------------------------------
    ("Open tarsier", "打开 tarsier"),
    ("Set up monitor inputs…", "设置显示器输入…"),
    ("Switch to", "切换到"),
    ("Take a break now", "现在休息"),
    ("Snooze this break", "推迟这次休息"),
    ("Skip this break", "跳过这次休息"),
    ("Resume reminders", "恢复提醒"),
    ("Pause reminders for 1 hour", "暂停提醒 1 小时"),
    ("Quit", "退出"),
    // ---- notices: what the last switch or save did ---------------------
    ("Switched to {label}", "已切换到 {label}"),
    ("Switching input failed: {e}", "切换输入失败: {e}"),
    ("Switch failed: {e}", "切换失败: {e}"),
    (
        "No computers are set up to switch between yet — add them on the Settings tab",
        "还没有设置要切换的电脑，请在「设置」页添加",
    ),
    (
        "Cannot read which input the monitor is on, so there is no direction to flip in — pick one in the quick-switch panel",
        "读不出显示器现在在哪一路，没法盲翻 —— 在快切面板里选一台。",
    ),
    ("Could not change the autostart setting: {e}", "设置开机启动失败: {e}"),
    ("Could not save settings: {e}", "保存设置失败: {e}"),
    (
        "The clipboard does not hold tarsier input-switch settings",
        "剪贴板里没有 tarsier 的输入切换设置",
    ),
    (
        "The clipboard settings do not match the monitors connected now",
        "剪贴板里的设置和当前接的显示器对不上",
    ),
    ("Applied to 1 monitor", "已应用到 1 台显示器"),
    ("Applied to {n} monitors", "已应用到 {n} 台显示器"),
    // ---- tray tooltip -------------------------------------------------
    ("Break reminders are off", "休息提醒已关闭"),
    ("Reminders are paused", "提醒已暂停"),
    ("Break in 1 minute", "1 分钟后休息"),
    ("Break in {n} minutes", "{n} 分钟后休息"),
    (
        "tarsier · {state} · {score} pts today",
        "tarsier · {state} · 今日 {score} 分",
    ),
    // ---- break overlay ------------------------------------------------
    ("Take a break", "休息一下吧"),
    (
        "You have been working for 1 minute. {tip}",
        "你已经连续工作了 1 分钟。{tip}",
    ),
    (
        "You have been working for {n} minutes. {tip}",
        "你已经连续工作了 {n} 分钟。{tip}",
    ),
    (
        "Relaxing… the countdown runs while you are away from the keyboard and mouse",
        "放松中…离开键盘鼠标，倒计时会自动走完",
    ),
    (
        "Keyboard or mouse activity detected — the countdown is paused",
        "检测到键鼠操作，倒计时已暂停",
    ),
    (
        "The overlay does not block input · to snooze or skip, right-click tarsier in the tray",
        "遮罩不会挡住操作 · 需要推迟或跳过，请右键托盘里的 tarsier",
    ),
    (
        "Look at something 6 metres away and let your eyes relax",
        "看看 6 米外的地方，让眼睛的睫状肌放松一下",
    ),
    (
        "Stand up, stretch, and roll your neck and shoulders",
        "站起来伸个懒腰，转转脖子和肩膀",
    ),
    (
        "Fetch a glass of water and walk around a little",
        "去倒杯水，顺便走动走动",
    ),
    ("Close your eyes and take a few slow breaths", "闭上眼睛，深呼吸几次"),
    ("Blink a few times to wet your eyes again", "眨眨眼，让眼睛重新湿润起来"),
    ("Move your wrists and fingers around", "活动一下手腕和手指"),
    // ---- quick-switch panel -------------------------------------------
    ("Switch monitor input", "切换显示器输入"),
    ("Press a number to jump straight there", "按数字键直达"),
    ("Esc to cancel", "Esc 取消"),
    // ---- statistics: titles earned by points ---------------------------
    ("Sedentary starter", "久坐新手"),
    ("Stretch apprentice", "伸展学徒"),
    ("Pacing pro", "节奏达人"),
    ("Eye guardian", "护眼卫士"),
    ("Health pro", "健康达人"),
    ("Tarsier grandmaster", "眼镜猴大师"),
    // ---- switching errors that reach a notice --------------------------
    (
        "No LG side-channel value is known for {port} — add one under input_protocol.values in the config",
        "不知道 {port} 在 LG 私有通道上的编号，请在配置的 input_protocol.values 里补充",
    ),
    (
        "This LG monitor only accepts input switching as I²C commands sent straight by the GPU, which this GPU does not support (NVIDIA today)",
        "这台 LG 显示器只能由显卡直接发 I²C 命令切换输入，当前显卡不支持（目前支持 NVIDIA）",
    ),
    (
        "Administrator permission was not granted, so the switch was cancelled",
        "没有获得管理员授权，已取消切换",
    ),
];

/// The main window's copy: the same table, kept in its own block because that
/// one window holds most of the strings in the program.
static MAIN_WINDOW: &[(&str, &str)] = &[
    // ---- break durations and the names offered during setup ------------
    ("Work", "每工作"),
    ("Break", "休息"),
    ("Snooze", "推迟"),
    ("min", "分钟"),
    ("Desktop", "台式机"),
    ("Work PC", "公司电脑"),
    ("Laptop", "笔记本"),
    ("Console", "游戏机"),
    // ---- monitors tab -------------------------------------------------
    ("Scanning…", "正在检测…"),
    ("Scan again", "重新检测"),
    ("No monitors detected", "未检测到显示器"),
    ("Connected to 1 DDC/CI monitor", "已连接 {n} 台支持 DDC/CI 的显示器"),
    ("Connected to {n} DDC/CI monitors", "已连接 {n} 台支持 DDC/CI 的显示器"),
    ("Copy diagnostics report", "复制诊断报告"),
    ("Diagnostics report copied to the clipboard", "诊断报告已复制到剪贴板"),
    (
        "No external monitors supporting DDC/CI were found",
        "没有找到支持 DDC/CI 的外接显示器",
    ),
    (
        "Turn on DDC/CI in the monitor's on-screen menu, then click \"Scan again\". Built-in laptop screens do not support DDC/CI.",
        "请在显示器的 OSD 菜单里开启 DDC/CI，然后点击「重新检测」。笔记本内置屏不支持 DDC/CI。",
    ),
    ("Not supported", "不支持"),
    ("Brightness", "亮度"),
    ("Contrast", "对比度"),
    // ---- switching between the computers on one monitor -----------------
    ("One-key switching", "一键切换"),
    ("Input switching", "输入切换"),
    (
        "No computers sharing this monitor are set up yet.",
        "还没设置共用这台显示器的电脑。",
    ),
    ("Set up on the Settings tab", "去设置里配置"),
    ("Switch to {name}", "切换到 {name}"),
    (
        "Press the hotkey to flip between the two, without looking up which one you are on first.",
        "按快捷键在两台之间来回切，不用先看现在在哪台。",
    ),
    (
        "Press the hotkey for the quick-switch panel and jump by number — it never passes through the machine in between.",
        "按快捷键呼出快切面板，按数字直达 —— 不会路过中间那台。",
    ),
    ("These ports have no names yet.", "这些接口还没有名字。"),
    ("Name them on the Settings tab", "去设置里起名"),
    (
        "The monitor does not report its input list, so these are the common ports; add more under extra_inputs in the config.",
        "显示器没有上报输入列表，这里列出的是常见接口；可在配置文件 extra_inputs 里补充。",
    ),
    // ---- first-run wizard ----------------------------------------------
    (
        "How many computers are connected to this monitor?",
        "这台显示器上接着几台电脑？",
    ),
    (
        "Select the ports that actually have a computer behind them. Ports that are empty, or that go to a game console or a TV box, can stay unselected.",
        "把真正接着电脑的口点亮。空着的口、接游戏机或电视盒子的口，都可以不选。",
    ),
    (
        "Assuming {port} is this computer — click another port if that is wrong",
        "默认把「{port}」算作这台电脑，不对的话点一下换个口。",
    ),
    (
        "The monitor reports no inputs, so click whichever port this computer is on",
        "显示器没有上报输入，自己点一下哪个口是这台电脑就行。",
    ),
    ("Next: name this 1 computer", "下一步：给这 {n} 台起名"),
    ("Next: name these {n} computers", "下一步：给这 {n} 台起名"),
    ("Pick at least two to switch with one key", "至少选两台才能一键切换"),
    ("Paste from another computer", "从另一台电脑粘贴"),
    ("Give them names", "给它们起个名字"),
    (
        "The names appear here, in the tray menu, and in the quick-switch panel — they are the only thing that lets you tell the machines apart at a glance.",
        "名字会出现在这里、托盘菜单和快切面板上 —— 这是唯一能让你一眼认出谁是谁的东西。",
    ),
    (
        "This computer takes its name from the system ({hostname}); for the rest, click a common name. Install tarsier on the other computers too and paste this set of names across, and the 3rd and 4th need no typing at all.",
        "这台电脑的名字自动取系统里的「{hostname}」，其余的点一下常用名就行。另一台电脑上也装一份 tarsier，把这套名字粘过去，第 3、第 4 台就都不用再填了。",
    ),
    ("Back", "上一步"),
    ("Done", "完成"),
    ("+ Add computer", "+ 添加电脑"),
    (
        "This monitor does not report an input list; add its ports under extra_inputs in the config first.",
        "这台显示器没有上报输入列表，先在配置文件的 extra_inputs 里补上接口。",
    ),
    // ---- breaks tab -----------------------------------------------------
    ("Turn them back on from the Settings tab", "可以在「设置」里重新打开"),
    ("You stepped away for a while 👋", "你离开了一会儿 👋"),
    ("A fresh timer starts when you come back", "回来后会开始新的一轮计时"),
    ("Taking a break", "正在休息"),
    ("Rest for another {time}", "还需休息 {time}"),
    ("Working for {time}", "已连续工作 {time}"),
    ("Break in {time}", "{time} 后提醒休息"),
    ("Snooze for 1 min", "推迟 {n} 分钟"),
    ("Snooze for {n} min", "推迟 {n} 分钟"),
    ("Today's health score", "今日健康分"),
    ("Scoring starts after 15 minutes of use", "使用 15 分钟后开始评分"),
    ("Screen time {time}", "用眼 {time}"),
    ("1 break", "休息 {n} 次"),
    ("{n} breaks", "休息 {n} 次"),
    ("Longest stretch {time}", "最长连续 {time}"),
    ("+{points} points today", "今日积分 +{points}"),
    (
        "Scoring: a session scores full marks up to 110% of {work} minutes, and loses more the longer it runs past that. Being away from the computer for {rest} minutes counts as a break automatically.",
        "评分规则：每段连续工作不超过 {work} 分钟的 110% 记满分，超得越多扣得越多。离开电脑 {rest} 分钟会被自动记为一次休息。",
    ),
    // ---- statistics tab --------------------------------------------------
    ("1 day", "{n} 天"),
    ("{n} days", "{n} 天"),
    ("Streak", "连续达标"),
    ("Total points", "累计积分"),
    ("Current title", "当前称号"),
    ("1 more point to level up", "再得 {n} 分升级"),
    ("{n} more points to level up", "再得 {n} 分升级"),
    ("Highest level reached", "已满级"),
    ("Today", "今天"),
    ("No completed sessions today", "今天还没有完成的工作段"),
    ("Natural break", "自然休息"),
    ("Prompted break", "提醒后休息"),
    ("{score} points", "{score} 分"),
    (
        "Today: {skipped} skipped, {snoozed} snoozed, {ignored} ignored",
        "今天跳过 {skipped} 次、推迟 {snoozed} 次、忽略 {ignored} 次提醒",
    ),
    ("Health score, last 7 days", "最近 7 天健康分"),
    ("Today's sessions", "今日工作段"),
    // ---- settings tab -----------------------------------------------------
    ("Appearance", "外观"),
    (
        "System tracks the Windows light / dark setting as you change it",
        "「跟随系统」会随 Windows 的浅色 / 深色设置实时切换",
    ),
    ("Language", "语言"),
    ("Switching redraws every window right away", "切换后所有窗口立即更新"),
    ("General", "通用"),
    ("Start automatically at login", "开机自动启动"),
    (
        "Runs quietly in the tray after you sign in to Windows",
        "登录 Windows 后在托盘里静默运行",
    ),
    ("Developer mode", "开发者模式"),
    (
        "Shows DDC/CI diagnostics and command tracing on the Monitors tab, and logs every command",
        "在「显示器」页显示 DDC/CI 诊断信息和命令记录，并在日志里记录每条命令",
    ),
    ("Break reminders", "休息提醒"),
    ("Turn on break reminders", "启用休息提醒"),
    (
        "When the time is up, a full-screen reminder fades in; stepping away counts as a break automatically",
        "到点后全屏淡入提醒，离开电脑会自动记为休息",
    ),
    ("Stay quiet in fullscreen / presentations", "全屏 / 演示时不打扰"),
    (
        "Holds reminders while you are gaming, watching video, or presenting",
        "玩游戏、看视频或演示 PPT 时推迟提醒",
    ),
    ("Not set", "未设置"),
    ("Monitor inputs", "显示器输入"),
    (
        "List the computers sharing each monitor and give them names — the names are the only thing that lets you tell them apart at a glance. Click the dot on the left to mark the one you are sitting at; it only affects what is shown, never switching.",
        "给每台显示器列出共用它的电脑并起好名字 —— 名字是唯一能让你一眼认出谁是谁的东西。点左端圆点标记你正坐着的那台，只影响显示，不影响切换。",
    ),
    (
        "No external monitors that support DDC/CI have been detected yet.",
        "还没有检测到支持 DDC/CI 的外接显示器。",
    ),
    (
        "The port-to-name mapping lives on the monitor, so it holds whichever computer you fill it in on. Set it up once and move it to the rest, and you never type it in again.",
        "「接口 → 电脑名」跟着显示器走，所以在哪台电脑上填都一样。在一台上填好，把它搬到其余几台，就不用再填一遍。",
    ),
    ("Copy settings", "复制设置"),
    (
        "Copied. Click \"Import settings\" on the other computer.",
        "已复制。在另一台电脑上点「导入设置」。",
    ),
    ("Import settings", "导入设置"),
    (
        "Global hotkeys (restart after editing the config file)",
        "全局快捷键（修改配置文件后重启生效）",
    ),
    ("Brightness up", "调高亮度"),
    ("Brightness down", "调低亮度"),
    ("Registration failed: {e}", "注册失败 {e}"),
    ("Open config folder", "打开配置文件夹"),
    // ---- tab bar and developer diagnostics -------------------------------
    ("Monitors", "显示器"),
    ("Breaks", "休息"),
    ("Stats", "统计"),
    ("Settings", "设置"),
    ("Diagnostics", "诊断信息"),
    ("Recent commands", "最近的命令"),
    ("(none)", "（暂无）"),
];

fn zh_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ZH.iter().chain(MAIN_WINDOW).copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every English key has to be unique: a duplicate would silently win over
    /// the other translation depending on which table it sat in.
    #[test]
    fn no_english_key_is_listed_twice() {
        let mut keys: Vec<&str> = ZH.iter().chain(MAIN_WINDOW).map(|(en, _)| *en).collect();
        let listed = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), listed, "a source string is listed twice");
    }

    /// A copy-paste that left the English on both sides would ship as a
    /// "translation" that does nothing.
    #[test]
    fn every_translation_actually_translates() {
        for (en, zh) in ZH.iter().chain(MAIN_WINDOW) {
            assert_ne!(en, zh, "{en:?} is its own translation");
            assert!(
                zh.chars().any(is_cjk),
                "{zh:?} has no Chinese in it, not even punctuation"
            );
        }
    }

    /// Han characters, CJK punctuation and the full-width forms: the separator
    /// is translated as "；", which is none of the first without being the rest.
    fn is_cjk(c: char) -> bool {
        matches!(c, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}')
    }

    #[test]
    fn chinese_punctuation_counts_as_chinese() {
        assert!(is_cjk('；'));
        assert!(is_cjk('显'));
        assert!(!is_cjk(';'));
        assert!(!is_cjk('A'));
    }

    /// Every `{name}` in a source string, braces included.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{')
            && let Some(len) = rest[start..].find('}')
        {
            found.push(&rest[start..start + len + 1]);
            rest = &rest[start + len + 1..];
        }
        found
    }

    /// Placeholders survive the lookup, or the translation would be printed
    /// with a hole in it.
    #[test]
    fn chinese_keeps_the_placeholders_it_is_filled_with() {
        for (en, zh) in ZH.iter().chain(MAIN_WINDOW) {
            for placeholder in placeholders(en) {
                assert!(
                    zh.contains(placeholder),
                    "{en:?} takes {placeholder} but its translation does not"
                );
            }
        }
    }

    #[test]
    fn placeholders_are_recognised() {
        assert_eq!(placeholders("Switched to {label}"), ["{label}"]);
        assert_eq!(placeholders("{n} h {m} min"), ["{n}", "{m}"]);
        assert!(placeholders("Quit").is_empty());
    }

    #[test]
    fn english_is_the_source_and_chinese_comes_from_the_table() {
        assert_eq!(tr_in(Language::En, "Switch monitor input"), "Switch monitor input");
        assert_eq!(tr_in(Language::Zh, "Switch monitor input"), "切换显示器输入");
    }

    /// An untranslated string must still read as words, not as a key.
    #[test]
    fn a_string_nobody_translated_falls_back_to_english() {
        assert_eq!(tr_in(Language::Zh, "Not translated yet"), "Not translated yet");
    }

    #[test]
    fn placeholders_are_filled_after_the_lookup() {
        let args = [("label", "Laptop".to_string())];
        assert_eq!(
            tr_args_in(Language::En, "Switched to {label}", &args),
            "Switched to Laptop"
        );
        assert_eq!(
            tr_args_in(Language::Zh, "Switched to {label}", &args),
            "已切换到 Laptop"
        );
    }

    /// Chinese needs the same translation for both forms; English must not say
    /// "1 monitors".
    #[test]
    fn the_counted_form_reads_correctly_in_both_languages() {
        let one = tr_args_in(Language::Zh, "Applied to 1 monitor", &[]);
        let many = tr_args_in(Language::Zh, "Applied to {n} monitors", &[("n", "3".to_string())]);
        assert_eq!(one, "已应用到 1 台显示器");
        assert_eq!(many, "已应用到 3 台显示器");
    }

    #[test]
    fn the_counted_form_picks_the_form_that_agrees_with_the_number() {
        assert_eq!(
            tr!(n = 1, "Applied to 1 monitor" | "Applied to {n} monitors"),
            "Applied to 1 monitor"
        );
        assert_eq!(
            tr!(n = 3, "Applied to 1 monitor" | "Applied to {n} monitors"),
            "Applied to 3 monitors"
        );

        // Counts arrive as whatever the caller counts in, and a count and
        // another placeholder can appear together.
        let worked: u64 = 2;
        assert_eq!(
            tr!(
                n = worked,
                "You have been working for 1 minute. {tip}" | "You have been working for {n} minutes. {tip}",
                tip = "Stand up"
            ),
            "You have been working for 2 minutes. Stand up"
        );
    }

    /// The language picker names languages in their own language, which is the
    /// one string that must never be translated.
    #[test]
    fn language_names_are_never_translated() {
        assert_eq!(Language::En.label(), "English");
        assert_eq!(Language::Zh.label(), "中文");
    }

    #[test]
    fn the_default_language_is_english() {
        assert_eq!(Language::default(), Language::En);
    }

    /// The names in `config.json`, which is hand-edited, are a contract with
    /// whatever is already on disk and with what the docs tell people to write.
    #[test]
    fn the_config_names_the_languages_en_and_zh() {
        assert_eq!(serde_json::to_string(&Language::En).unwrap(), "\"en\"");
        assert_eq!(serde_json::to_string(&Language::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::from_str::<Language>("\"zh\"").unwrap(), Language::Zh);
    }

    /// The component library has strings of its own — the cut/copy/paste menu a
    /// text field opens — and a locale of its own, spelled differently from
    /// ours. A switch that only moved our tables would leave those in English.
    ///
    /// It is a process global, so this test puts it back, and it deliberately
    /// drives the library directly rather than through [`set_language`]: that
    /// one flips our own global too, and the rest of the suite reads it.
    #[test]
    fn the_language_reaches_the_component_library_too() {
        gpui_kit::component::set_locale(Language::Zh.locale());
        assert_eq!(&*gpui_kit::component::locale(), "zh-CN");
        gpui_kit::component::set_locale(Language::En.locale());
        assert_eq!(&*gpui_kit::component::locale(), "en");
    }

    /// Every `.rs` file under `src/` except this one, concatenated.
    fn source_text() -> String {
        fn walk(dir: &std::path::Path, out: &mut String) {
            for entry in std::fs::read_dir(dir).expect("reading src/").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "i18n.rs")
                {
                    out.push_str(&std::fs::read_to_string(&path).expect("reading a source file"));
                }
            }
        }
        let mut out = String::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        // An escape stands for the character it denotes: a key containing a
        // quotation mark is written `\"` where it is used.
        out.replace("\\\"", "\"")
    }

    /// Keys are source strings, so nothing at compile time ties a call site to
    /// the table. Reword a string and its translation is stranded — the entry
    /// stays, the new wording is silently English in Chinese. This walks the
    /// sources and fails when a key is no longer written anywhere.
    #[test]
    fn every_key_still_appears_in_the_source() {
        let source = source_text();
        for (en, _) in ZH.iter().chain(MAIN_WINDOW) {
            assert!(
                source.contains(en),
                "{en:?} is in the table but is no longer written anywhere in src/"
            );
        }
    }
}
