# 构建与运行

[English](building.en.md) · **简体中文** · [返回 README](../README.md)

## 从源码构建

需要 Rust（版本见 `mise.toml`）和 MSVC 工具链 / Windows SDK。

```bash
cargo build --release
```

产物为 `target/release/tarsier.exe`。release 版带 `windows_subsystem = "windows"`，不会弹控制台窗口。

开发时用 `cargo run` 会保留控制台，日志直接打在终端里。

## 命令行参数

| 参数 | 作用 |
| --- | --- |
| `--background` | 启动时只进托盘，不显示主窗口。开机自启动写入的就是这一条。 |
| `--raw-ddc-write` | 提权助手模式，见下。 |

## 单实例

tarsier 通过命名互斥体保证单实例。重复启动时新进程会直接退出，并把已有实例的主窗口唤到前台。

## 提权助手（`--raw-ddc-write`）

Intel 驱动只允许管理员权限的进程写 I²C。tarsier 本身不以管理员身份运行，需要走 Intel 私有通道时，它会用管理员身份临时启动一个只发这一条命令的子进程，发完即退。

这个子进程路径在 `main` 的最前面处理：它不碰单实例锁，也不开任何窗口，只跑一次 DDC/CI 写入就退出。

## 开机自启动

写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，不是计划任务，所以不需要管理员权限。设置页的开关和它直接对应。
