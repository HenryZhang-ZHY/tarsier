# tarsier

[English](README.md) · **简体中文**

管理显示器和屏幕时间的 Windows 小工具，用 Rust + [GPUI](https://gpui.rs)（[gpui-kit](https://gpui-kit.com) 组件库）编写。灵感来自 Twinkle Tray 和 Fadetop。

常驻托盘，自绘标题栏，支持亮 / 暗 / 跟随系统三种主题，界面语言可切英文或中文。

## 皮肤

窗口有两套视觉语言，在设置页切换：

- **默认** —— tarsier 一直以来的样子：系统字体、圆角、克制的灰色。
- **新粗野主义（Neo Brutalism）** —— 奶油纸张、纯黑墨水、粗黑描边、硬位移阴影，标题字体是内置的 Space Grotesk。按设计只提供浅色：它是唯一一种确定的浅色配色，而不是"碰巧很亮"的主题。

皮肤管的是整套视觉语言，不只是颜色——画布和它的网格、面板、字号层级、徽章、按钮都归它，所以两套不会互相串味。要加一套皮肤，只需要写一个实现 `SkinStyle` 的文件；界面代码一行都不用动，因为它要的是「一张卡片」或「一个强调按钮」，而不是「4px 描边」。

休息遮罩和快切面板会跟随当前皮肤的配色，但保留各自的结构：一个是全屏变暗层，一个是紧凑弹窗，两者都不该长成卡片的样子。

## 功能

- **亮度 / 对比度**：通过 DDC/CI 直接调节外接显示器。
- **一键切换输入源**：给每台显示器列出接着的电脑，按快捷键或点托盘菜单切过去。两台时是盲翻，三台起会先问你去哪台。不认标准命令的显示器（比如新款 LG）会自动改用厂商私有通道。
- **休息提醒**：连续工作到设定时长后，像 Fadetop 一样在所有屏幕上淡入一层半透明遮罩；遮罩不抢焦点，鼠标点击直接穿过去。倒计时只在你真的离开键盘鼠标时才走。
- **打分激励**：每段工作按时长打分，另有积分、称号和连续达标天数。

## 安装

从 [Releases](https://github.com/HenryZhang-ZHY/tarsier/releases) 下载 `tarsier.exe` 直接运行即可，无需安装。

## 使用

启动后驻留托盘，双击托盘图标打开主窗口。

| 快捷键 | 作用 |
| --- | --- |
| `Ctrl+Alt+I` | 切换显示器输入。两台电脑时直接盲翻，三台以上弹出快切面板，按数字直达 |
| `Ctrl+Alt+PageUp` / `PageDown` | 所有显示器亮度 ±10% |

主窗口有三个标签页：**显示器**、**休息与统计**、**设置**；设置页左侧再分成通用、休息、显示器、快捷键、高级几个分区，想改哪一项直接点它的名字，不用在一条长列表里翻。

**快捷键在界面里录制**：点某一行的「录制」，按下组合键，立即生效 —— 不用改配置文件，也不用重启。其余设置都在设置页，或配置文件 `%APPDATA%\tarsier\config.json` 里。

## 文档

- [构建与运行](docs/building.zh.md) — 从源码编译，命令行参数
- [配置参考](docs/configuration.zh.md) — `config.json` 全部字段
- [厂商私有通道](docs/private-channels.zh.md) — 为什么新款 LG 要绕道显卡驱动，以及 UAC 提权
- [休息提醒与评分](docs/breaks.zh.md) — 提醒规则、推迟、忽略、健康分怎么算
- [开发者模式](docs/developer-mode.zh.md) — DDC/CI 诊断和命令追踪
- [已知限制](docs/limitations.zh.md) — 什么情况调不了

## 许可

MIT。图标来自 [Lucide](https://lucide.dev)（ISC）。新粗野主义皮肤内置了 [Space Grotesk](https://fonts.google.com/specimen/Space+Grotesk) 字体（SIL Open Font License 1.1，许可证与来源说明在 `assets/fonts/`）。
