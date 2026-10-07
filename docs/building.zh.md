# 构建与运行

[English](building.md) · **简体中文** · [返回 README](../README.zh.md)

## 从源码构建

需要 Rust（版本见 `mise.toml`）和 MSVC 工具链 / Windows SDK。

```bash
cargo build --release
```

产物为 `target/release/tarsier.exe`。release 版带 `windows_subsystem = "windows"`，不会弹控制台窗口。

开发时用 `cargo run` 会保留控制台，日志直接打在终端里。

## 测试

```bash
cargo test
```

除了单元测试，主窗口还会在无头 GPUI 窗口里测试（gpui-kit 的 `test-support`）：真实的布局、点击和按键，只是不出像素。这些测试守住了肉眼最容易弄坏的地方——标签栏在窗口正中、最窄窗口下没有控件被挤出右边缘、每个页面都能滚到底并从顶部打开——两种皮肤、两种语言都覆盖。另有测试在 `tr!` 字符串缺少中文翻译、或翻译对应的字符串已不再使用时失败。

## 独立的数据目录

设置 `TARSIER_HOME`（或传入 `--home <目录>`），配置、统计和日志就会放在 `%APPDATA%\tarsier` 之外。这一份有自己的单实例锁和自己的开机启动项，开发版可以和已安装的版本同时运行，互不干扰：

```powershell
$env:TARSIER_HOME = "$PWD\.dev-home"; cargo run
```

## 命令行参数

| 参数 | 作用 |
| --- | --- |
| `--background` | 启动时只进托盘，不显示主窗口。开机自启动写入的就是这一条。 |
| `--home <目录>` | 数据放在 `<目录>`，作用同 `TARSIER_HOME`。数据目录独立的那一份开机启动时会带上它。 |
| `--raw-ddc-write` | 提权助手模式，见下。 |

## 单实例

tarsier 通过命名互斥体保证单实例。重复启动时新进程会直接退出，并把已有实例的主窗口唤到前台。

## 提权助手（`--raw-ddc-write`）

Intel 驱动只允许管理员权限的进程写 I²C。tarsier 本身不以管理员身份运行，需要走 Intel 私有通道时，它会用管理员身份临时启动一个只发这一条命令的子进程，发完即退。

这个子进程路径在 `main` 的最前面处理：它不碰单实例锁，也不开任何窗口，只跑一次 DDC/CI 写入就退出。

## 开机自启动

写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，不是计划任务，所以不需要管理员权限。设置页的开关和它直接对应。
