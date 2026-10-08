//! The language the UI is drawn in.
//!
//! English is the source language. Every user-facing string is written in
//! English in the code and wrapped in [`tr!`], which looks the text up in the
//! tables below when the user has picked Chinese. A string with no entry falls
//! back to its English source, so a forgotten translation shows up as English
//! rather than as an empty label or a raw key — and the debug log names it.
//!
//! Adding a string: write it in English, wrap it in [`tr!`], and add the pair
//! to [`ZH`]. The tests scan `src/` for every `tr!` literal, so a string
//! without a translation, or a translation whose string is gone, fails the
//! build rather than shipping as English in the Chinese UI.
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

    /// Stable across languages, for element ids.
    pub fn key(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
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

/// The language every window draws in. Written from the saved config at
/// startup and whenever the user changes it; read on the UI thread.
#[cfg(not(test))]
static CURRENT: AtomicU8 = AtomicU8::new(Language::En as u8);

// Tests run in parallel and some draw the window in Chinese, so each test
// thread keeps a language of its own.
#[cfg(test)]
thread_local! {
    static CURRENT: AtomicU8 = const { AtomicU8::new(Language::En as u8) };
}

fn store(language: Language) {
    #[cfg(not(test))]
    CURRENT.store(language as u8, Ordering::Relaxed);
    #[cfg(test)]
    CURRENT.with(|c| c.store(language as u8, Ordering::Relaxed));
}

fn load() -> u8 {
    #[cfg(not(test))]
    return CURRENT.load(Ordering::Relaxed);
    #[cfg(test)]
    return CURRENT.with(|c| c.load(Ordering::Relaxed));
}

/// Switches the whole program over, wherever the strings come from.
///
/// Ours are looked up at draw time, so nothing has to be told about the change
/// beyond a repaint. The component library keeps its own strings and has to be
/// told separately; every caller comes through here, which keeps the two in
/// step.
pub fn set_language(language: Language) {
    store(language);
    gpui_kit::component::set_locale(language.locale());
}

pub fn language() -> Language {
    if load() == Language::Zh as u8 {
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

/// English source text → 简体中文. A placeholder in braces stays as it is;
/// a counted string lists both its forms, which share a translation.
static ZH: &[(&str, &str)] = &[
    // ---- shared vocabulary -------------------------------------------------
    ("{n} h {m} min", "{n} 小时 {m} 分钟"),
    ("{n} min", "{n} 分钟"),
    ("Light", "亮色"),
    ("Dark", "暗色"),
    ("System", "跟随系统"),
    ("Default", "默认"),
    ("Neo Brutalism", "新粗野主义"),
    (
        "The system font, soft corners and quiet greys, in light or dark",
        "系统字体、柔和圆角和安静的灰色，有亮色和暗色",
    ),
    (
        "Warm paper, black ink, solid strokes and hard shadows. Light only, by design",
        "暖纸色、黑墨线、实心描边和硬阴影。只有亮色，这是有意的设计",
    ),
    // ---- tray menu and tooltip ---------------------------------------------
    ("Open tarsier", "打开 tarsier"),
    ("Set up monitor inputs…", "设置显示器输入…"),
    ("Switch to", "切换到"),
    ("Take a break now", "现在休息"),
    ("Snooze this break", "推迟这次休息"),
    ("Skip this break", "跳过这次休息"),
    ("Resume reminders", "恢复提醒"),
    ("Pause reminders for 1 hour", "暂停提醒 1 小时"),
    ("Quit", "退出"),
    // ---- notices from the controller ---------------------------------------
    ("Switched to {label}", "已切换到 {label}"),
    ("Switching input failed: {e}", "切换输入失败: {e}"),
    (
        "No computers are set up to switch between yet. Add them in Settings → Displays.",
        "还没有设置要切换的电脑，请在「设置 → 显示器」中添加。",
    ),
    (
        "The monitor will not say which input it is on, so pick one in the quick-switch panel.",
        "显示器没有告知当前输入，请在快切面板中选择。",
    ),
    ("; ", "；"),
    ("Could not change the autostart setting: {e}", "设置开机启动失败: {e}"),
    ("Could not save settings: {e}", "保存设置失败: {e}"),
    (
        "The clipboard does not hold tarsier input-switch settings",
        "剪贴板里没有 tarsier 的输入切换设置",
    ),
    (
        "The copied settings do not match any monitor connected here",
        "复制的设置与这台电脑连接的显示器都不匹配",
    ),
    ("Applied to 1 monitor", "已应用到 1 台显示器"),
    ("Applied to {n} monitors", "已应用到 {n} 台显示器"),
    ("Break reminders are off", "休息提醒已关闭"),
    ("Reminders are paused", "提醒已暂停"),
    ("On a break", "休息中"),
    ("Away", "已离开"),
    ("Break in 1 minute", "1 分钟后休息"),
    ("Break in {n} minutes", "{n} 分钟后休息"),
    (
        "tarsier · {state} · health {score} today",
        "tarsier · {state} · 今日健康分 {score}",
    ),
    // ---- monitor protocol errors -------------------------------------------
    (
        "No LG side-channel value is known for {port} — add one under input_protocol.values in the config",
        "不知道 {port} 在 LG 私有通道上的编号，请在配置的 input_protocol.values 里补充",
    ),
    (
        "This LG monitor only accepts input switching as I²C commands sent straight by the GPU, which this GPU does not support (NVIDIA today)",
        "这台 LG 显示器只能由显卡直接发 I²C 命令切换输入，当前显卡不支持（目前支持 NVIDIA）",
    ),
    // ---- break overlay and quick-switch panel ------------------------------
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
    ("Switch monitor input", "切换显示器输入"),
    ("Press a number to jump straight there", "按数字键直达"),
    ("Esc to cancel", "Esc 取消"),
    // ---- main window: shell and Monitors tab -------------------------------
    ("Monitors", "显示器"),
    ("Settings", "设置"),
    ("Looking for monitors…", "正在查找显示器…"),
    ("No monitors detected", "未检测到显示器"),
    ("1 monitor with DDC/CI", "{n} 台支持 DDC/CI 的显示器"),
    ("{n} monitors with DDC/CI", "{n} 台支持 DDC/CI 的显示器"),
    ("Copy diagnostics", "复制诊断报告"),
    ("Diagnostics report copied to the clipboard", "诊断报告已复制到剪贴板"),
    (
        "No external monitors supporting DDC/CI were found",
        "没有找到支持 DDC/CI 的外接显示器",
    ),
    (
        "Turn on DDC/CI in the monitor's on-screen menu, then scan again. Built-in laptop screens do not support DDC/CI.",
        "请在显示器的屏幕菜单里打开 DDC/CI，然后重新检测。笔记本内置屏幕不支持 DDC/CI。",
    ),
    ("Scanning…", "正在检测…"),
    ("Scan again", "重新检测"),
    ("Brightness", "亮度"),
    ("Contrast", "对比度"),
    ("Not supported by this monitor", "这台显示器不支持"),
    ("Input switching", "输入切换"),
    (
        "No computers sharing this monitor are set up yet.",
        "还没设置共用这台显示器的电脑。",
    ),
    ("Set up switching", "设置切换"),
    ("Switch", "切换"),
    (
        "The hotkey flips between the two without asking which one you are on.",
        "快捷键会在两台之间直接切换，不用先确认当前是哪台。",
    ),
    (
        "The hotkey opens a quick-switch panel; press a number to jump straight there.",
        "快捷键会打开快切面板，按数字键直达。",
    ),
    (
        "These are port names, not computer names.",
        "这些是接口名，不是电脑名。",
    ),
    ("Name them", "起名"),
    ("Diagnostics", "诊断信息"),
    ("Recent commands", "最近的命令"),
    ("(none)", "（暂无）"),
    // ---- main window: Breaks tab -------------------------------------------
    (
        "Turn them back on in Settings to get a reminder after each stretch of work.",
        "在设置中重新打开后，每段工作结束时都会提醒你休息。",
    ),
    ("You stepped away", "你离开了一会儿"),
    ("A fresh timer starts when you come back.", "回来后会重新开始计时。"),
    ("Taking a break", "正在休息"),
    (
        "The countdown runs while you are away from the keyboard and mouse.",
        "离开键盘和鼠标时才会倒计时。",
    ),
    ("Working for {time}", "已连续工作 {time}"),
    ("Reminders are paused for the next hour.", "接下来一小时内不会提醒。"),
    (
        "When the time is up, a reminder fades in over every screen. Stepping away counts as a break on its own.",
        "时间一到，所有屏幕上会渐渐浮现提醒。自己离开电脑也算休息。",
    ),
    ("{time} left", "还剩 {time}"),
    ("Break in {time}", "{time} 后提醒休息"),
    ("Snooze for 1 min", "推迟 {n} 分钟"),
    ("Snooze for {n} min", "推迟 {n} 分钟"),
    ("Pause for 1 hour", "暂停 1 小时"),
    (
        "Every {work} min of work, a {rest} min break. Snoozing waits {snooze} min.",
        "每工作 {work} 分钟休息 {rest} 分钟，推迟一次等 {snooze} 分钟。",
    ),
    ("Change", "修改"),
    ("1 point to the next title", "离下一个称号还差 {n} 分"),
    ("{n} points to the next title", "离下一个称号还差 {n} 分"),
    ("The highest title there is", "已经是最高称号"),
    ("Streak", "连续达标"),
    ("1 day", "{n} 天"),
    ("{n} days", "{n} 天"),
    ("Points", "积分"),
    ("Evening cutoff", "下机时间"),
    ("Stop using the computer in the evening", "晚上到点下机"),
    (
        "From the time you set until 05:00, an overlay over every screen says the day is done. You can always carry on.",
        "从你设定的时间到早上 5 点，所有屏幕上会出现一层遮罩，告诉你今天该结束了。想继续用，随时可以。",
    ),
    ("No computer after", "每天几点以后不用电脑"),
    (
        "Sleep doctors suggest putting screens away 30 to 60 minutes before bed. Try an hour before you usually go to bed.",
        "睡眠专家建议睡前 30–60 分钟停用屏幕。可以从你平时上床的时间往前推一小时。",
    ),
    ("Title", "称号"),
    ("Grade {g}", "评级 {g}"),
    ("out of 100", "满分 100"),
    ("Scoring starts after 15 minutes of use", "使用 15 分钟后开始评分"),
    ("Screen time", "屏幕时间"),
    ("Breaks", "休息"),
    ("Longest stretch", "最长连续工作"),
    ("Points today", "今日积分"),
    ("Today", "今天"),
    ("Last 7 days", "最近 7 天"),
    ("Good ({n}+)", "达标（{n} 分及以上）"),
    ("Below {n}", "低于 {n} 分"),
    ("No data", "未使用"),
    (
        "A session scores full marks up to 110% of {work} minutes and loses more the longer it runs past that. {rest} minutes away from the computer counts as a break.",
        "一段工作不超过 {work} 分钟的 110% 就得满分，超出越多扣得越多。离开电脑 {rest} 分钟就算一次休息。",
    ),
    ("Stepped away", "自己离开"),
    ("Reminded", "提醒后休息"),
    (
        "Today: {skipped} skipped, {snoozed} snoozed, {ignored} ignored",
        "今天跳过 {skipped} 次、推迟 {snoozed} 次、忽略 {ignored} 次提醒",
    ),
    ("Sessions today", "今天的工作段"),
    ("No session has ended yet today.", "今天还没有结束的工作段。"),
    // ---- main window: Settings tab -----------------------------------------
    ("General", "通用"),
    ("Displays", "显示器"),
    ("Advanced", "高级"),
    ("Appearance", "外观"),
    ("Skin", "皮肤"),
    ("Light or dark", "亮色或暗色"),
    (
        "This skin is drawn in light only. Your choice comes back with the default skin.",
        "这款皮肤只有亮色，换回默认皮肤后会恢复你的选择。",
    ),
    (
        "System follows the Windows setting as it changes.",
        "「跟随系统」会随 Windows 设置实时切换。",
    ),
    ("Language", "语言"),
    ("Every window switches right away.", "所有窗口会立即切换。"),
    ("Start at login", "登录时启动"),
    (
        "Runs quietly in the tray after you sign in to Windows.",
        "登录 Windows 后在托盘里安静运行。",
    ),
    ("min", "分钟"),
    ("Break reminders", "休息提醒"),
    ("Remind me to take breaks", "提醒我休息"),
    (
        "After each stretch of work, a reminder fades in over every screen. It never blocks the keyboard or mouse.",
        "每段工作结束后，所有屏幕上会浮现提醒，不会挡住键盘和鼠标。",
    ),
    ("Stay quiet in fullscreen", "全屏时保持安静"),
    (
        "Holds reminders while you are gaming, watching video or presenting.",
        "玩游戏、看视频或演示时暂不提醒。",
    ),
    ("Developer mode", "开发者模式"),
    (
        "Shows DDC/CI diagnostics and recent commands on the Monitors tab, and logs every command.",
        "在显示器页显示 DDC/CI 诊断信息和最近的命令，并记录每条命令。",
    ),
    ("Settings folder", "设置文件夹"),
    ("Open", "打开"),
    ("Press a combination…", "按下组合键…"),
    ("Not set", "未设置"),
    ("Cancel", "取消"),
    ("Record", "录制"),
    ("Clear", "清除"),
    (
        "Another program is already using this combination.",
        "这个组合键已被其他程序占用。",
    ),
    (
        "This is not a combination Windows understands. Record it again.",
        "Windows 无法识别这个组合键，请重新录制。",
    ),
    (
        "Windows refused this combination: {reason}",
        "Windows 拒绝了这个组合键：{reason}",
    ),
    ("Hotkeys", "快捷键"),
    (
        "Click Record and press the combination you want. It works straight away, in every application.",
        "点击「录制」后按下想要的组合键，立即生效，在任何程序里都能用。",
    ),
    (
        "List the computers sharing each monitor and give them names; the names are what the tray menu and the quick-switch panel show. Marking which one you are sitting at only changes what is shown, never where a switch goes.",
        "列出共用每台显示器的电脑并给它们起名，托盘菜单和快切面板里显示的就是这些名字。标记你正在用哪台只影响显示，不会改变切换的去向。",
    ),
    ("No monitors to set up", "没有可设置的显示器"),
    (
        "Monitors that support DDC/CI appear here once they are detected.",
        "检测到支持 DDC/CI 的显示器后，会显示在这里。",
    ),
    ("Not set up", "未设置"),
    ("One-key flip", "盲切"),
    ("Quick-switch panel", "快切面板"),
    (
        "This monitor does not report its inputs. Add its ports under extra_inputs in the config file.",
        "这台显示器没有报告输入列表，请在配置文件的 extra_inputs 中添加它的接口。",
    ),
    ("Preview the quick-switch panel", "预览快切面板"),
    ("This PC", "本机"),
    ("Name this computer", "给这台电脑起名"),
    ("Name these {n} computers", "给这 {n} 台电脑起名"),
    ("Pick at least two", "至少选择两个"),
    ("Which ports have a computer behind them?", "哪些接口接着电脑？"),
    (
        "Leave out ports that are empty or go to a game console or TV box.",
        "空着的接口，以及接游戏机或电视盒子的接口，都不用选。",
    ),
    (
        "{port} looks like this computer, because the monitor is showing it now.",
        "显示器正在显示 {port}，所以它应该就是这台电脑。",
    ),
    (
        "The monitor does not say which input it is showing.",
        "显示器没有告知正在显示哪个输入。",
    ),
    ("Back", "上一步"),
    ("Done", "完成"),
    ("Give each computer a name", "给每台电脑起个名字"),
    (
        "This computer is named after the system ({hostname}). For the others, type a name or pick a common one.",
        "这台电脑使用系统名称（{hostname}）。其他电脑可以输入名字，或选一个常用的。",
    ),
    ("Remove this computer", "移除这台电脑"),
    ("Add a computer", "添加电脑"),
    (
        "The monitor does not report its inputs, so these are the common ports. Add others under extra_inputs in the config file.",
        "显示器没有报告输入列表，这里列的是常见接口。其他接口请在配置文件的 extra_inputs 中添加。",
    ),
    ("This is me", "这是本机"),
    ("Paste from another computer", "从另一台电脑粘贴"),
    ("Other computers", "其他电脑"),
    (
        "Set the names up once, copy them here, and paste them on each of the other computers.",
        "名字只需设置一次：在这里复制，再到其他每台电脑上粘贴。",
    ),
    ("Copy these settings", "复制这些设置"),
    (
        "Copied. Paste them on the other computer.",
        "已复制，请到另一台电脑上粘贴。",
    ),
    // ---- labels kept in tables and translated where drawn ------------------
    (
        "Administrator permission was not granted, so the switch was cancelled",
        "没有获得管理员授权，已取消切换",
    ),
    ("Blink a few times to wet your eyes again", "眨眨眼，让眼睛重新湿润起来"),
    ("Break", "休息"),
    ("Brightness down", "调低亮度"),
    ("Brightness up", "调高亮度"),
    ("Close your eyes and take a few slow breaths", "闭上眼睛，深呼吸几次"),
    ("Console", "游戏机"),
    ("Desktop", "台式机"),
    ("Eye guardian", "护眼卫士"),
    (
        "Fetch a glass of water and walk around a little",
        "去倒杯水，顺便走动走动",
    ),
    ("Health pro", "健康达人"),
    ("Laptop", "笔记本"),
    (
        "Look at something 6 metres away and let your eyes relax",
        "看看 6 米外的地方，让眼睛的睫状肌放松一下",
    ),
    ("Move your wrists and fingers around", "活动一下手腕和手指"),
    ("Pacing pro", "节奏达人"),
    ("Sedentary starter", "久坐新手"),
    ("Snooze", "推迟"),
    (
        "Stand up, stretch, and roll your neck and shoulders",
        "站起来伸个懒腰，转转脖子和肩膀",
    ),
    ("Stretch apprentice", "伸展学徒"),
    ("Tarsier grandmaster", "眼镜猴大师"),
    ("Work", "每工作"),
    ("Work PC", "公司电脑"),
];

fn zh_table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ZH.iter().copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_english_key_is_listed_twice() {
        let mut keys: Vec<&str> = ZH.iter().map(|(en, _)| *en).collect();
        let listed = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), listed, "a source string is listed twice");
    }

    /// A copy-paste that left the English on both sides would ship as a
    /// "translation" that does nothing.
    #[test]
    fn every_translation_actually_translates() {
        let cjk = |c: char| matches!(c, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}');
        for (en, zh) in ZH {
            assert!(zh.chars().any(cjk), "{en:?} → {zh:?} has no Chinese in it");
        }
    }

    /// Every `{name}` in a string, braces included, sorted.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{')
            && let Some(len) = rest[start..].find('}')
        {
            found.push(&rest[start..start + len + 1]);
            rest = &rest[start + len + 1..];
        }
        found.sort_unstable();
        found
    }

    /// A translation has exactly the placeholders it will be filled with, or it
    /// is printed with a hole in it. The counted form's singular names its
    /// number in words, so its translation may add `{n}` but nothing else.
    #[test]
    fn translations_keep_their_placeholders() {
        for (en, zh) in ZH {
            let mut want = placeholders(en);
            let got = placeholders(zh);
            if got.contains(&"{n}") && !want.contains(&"{n}") {
                want.push("{n}");
                want.sort_unstable();
            }
            assert_eq!(want, got, "{en:?} → {zh:?}");
        }
    }

    #[test]
    fn chinese_comes_from_the_table_and_falls_back_to_english() {
        assert_eq!(tr_in(Language::En, "Switch monitor input"), "Switch monitor input");
        assert_eq!(tr_in(Language::Zh, "Switch monitor input"), "切换显示器输入");
        // An untranslated string still reads as words, not as a key.
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

    #[test]
    fn the_counted_form_agrees_with_the_number() {
        assert_eq!(
            tr!(n = 1, "Applied to 1 monitor" | "Applied to {n} monitors"),
            "Applied to 1 monitor"
        );
        assert_eq!(
            tr!(n = 3, "Applied to 1 monitor" | "Applied to {n} monitors"),
            "Applied to 3 monitors"
        );
        // A count and another placeholder together, the count in any integer type.
        let worked: u64 = 2;
        assert_eq!(
            tr!(
                n = worked,
                "You have been working for 1 minute. {tip}" | "You have been working for {n} minutes. {tip}",
                tip = "Stand up"
            ),
            "You have been working for 2 minutes. Stand up"
        );
        // Chinese has no plural, so the singular carries the number too.
        assert_eq!(
            tr_args_in(Language::Zh, "Applied to 1 monitor", &[("n", "1".into())]),
            "已应用到 1 台显示器"
        );
    }

    #[test]
    fn the_ui_language_is_per_thread_in_tests_and_reaches_every_lookup() {
        set_language(Language::Zh);
        assert_eq!(translate("Quit"), "退出");
        let other = std::thread::spawn(|| translate("Quit")).join().unwrap();
        assert_eq!(other, "Quit", "another test thread is not affected");
        set_language(Language::En);
        assert_eq!(translate("Quit"), "Quit");
    }

    /// `config.json` is hand-edited, so the names in it are a contract with
    /// what is already on disk and with what the docs tell people to write.
    #[test]
    fn the_config_names_the_languages_en_and_zh() {
        assert_eq!(serde_json::to_string(&Language::En).unwrap(), "\"en\"");
        assert_eq!(serde_json::from_str::<Language>("\"zh\"").unwrap(), Language::Zh);
        assert_eq!(Language::default(), Language::En);
    }

    /// Every `.rs` file under `src/` but this one, with its comment lines
    /// dropped (doc examples name strings that are not in the UI).
    fn sources() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("reading src/").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "i18n.rs")
                {
                    let text = std::fs::read_to_string(&path).expect("reading a source file");
                    out.push(
                        text.lines()
                            .filter(|l| !l.trim_start().starts_with("//"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    /// The string literals passed straight to a `tr!` — the sources, both forms
    /// of a counted one — and not literals nested in its arguments.
    fn tr_literals(source: &str) -> Vec<String> {
        let chars: Vec<char> = source.chars().collect();
        let mut found = Vec::new();
        let mut i = 0;
        while i + 4 <= chars.len() {
            let start_of_word = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
            if !(start_of_word && chars[i..i + 4] == ['t', 'r', '!', '(']) {
                i += 1;
                continue;
            }
            i += 4;
            let mut depth = 1;
            while depth > 0 {
                match chars[i] {
                    '"' => {
                        let mut text = String::new();
                        i += 1;
                        while chars[i] != '"' {
                            if chars[i] == '\\' {
                                i += 1;
                                text.push(match chars[i] {
                                    'n' => '\n',
                                    other => other,
                                });
                            } else {
                                text.push(chars[i]);
                            }
                            i += 1;
                        }
                        if depth == 1 {
                            found.push(text);
                        }
                    }
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
        }
        found
    }

    #[test]
    fn the_scanner_finds_sources_and_skips_nested_literals() {
        let found = tr_literals(r#"x(tr!("A {e}", e = format!("{e:#}"))); tr!(n = k, "1 b" | "{n} b"); attr!("no")"#);
        assert_eq!(found, ["A {e}", "1 b", "{n} b"]);
    }

    /// Reword a string and nothing at compile time notices that its translation
    /// is now stranded — the Chinese UI silently shows English. So every string
    /// the code asks for has an entry...
    #[test]
    fn every_string_the_ui_asks_for_has_a_translation() {
        let table = zh_table();
        let mut missing: Vec<String> = sources()
            .iter()
            .flat_map(|s| tr_literals(s))
            .filter(|s| !table.contains_key(s.as_str()))
            .collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "no Chinese for: {missing:#?}");
    }

    /// ...and every entry is still asked for, through `tr!` or as a quoted
    /// label in a table that is translated where it is drawn.
    #[test]
    fn every_translation_is_still_used() {
        let sources = sources();
        let asked: std::collections::HashSet<String> = sources.iter().flat_map(|s| tr_literals(s)).collect();
        let quoted = sources.join("\n");
        for (en, _) in ZH {
            let literal = format!("\"{}\"", en.replace('\\', "\\\\").replace('"', "\\\""));
            assert!(
                asked.contains(*en) || quoted.contains(&literal),
                "{en:?} is in the table but nothing in src/ asks for it"
            );
        }
    }
}
