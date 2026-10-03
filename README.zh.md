# tarsier

[English](README.md) · **简体中文**

管理显示器和屏幕时间的 Windows 小工具，用 Rust + [GPUI](https://gpui.rs)（[gpui-kit](https://gpui-kit.com) 组件库）编写。灵感来自 Twinkle Tray 和 Fadetop。

常驻托盘，自绘标题栏，支持亮 / 暗 / 跟随系统三种主题。

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

快捷键、休息时长等都在主窗口的「设置」页，或配置文件 `%APPDATA%\tarsier\config.json` 里调整。

## 文档

- [构建与运行](docs/building.zh.md) — 从源码编译，命令行参数
- [配置参考](docs/configuration.zh.md) — `config.json` 全部字段
- [厂商私有通道](docs/private-channels.zh.md) — 为什么新款 LG 要绕道显卡驱动，以及 UAC 提权
- [休息提醒与评分](docs/breaks.zh.md) — 提醒规则、推迟、忽略、健康分怎么算
- [开发者模式](docs/developer-mode.zh.md) — DDC/CI 诊断和命令追踪
- [已知限制](docs/limitations.zh.md) — 什么情况调不了

## 许可

MIT。图标来自 [Lucide](https://lucide.dev)（ISC）。
