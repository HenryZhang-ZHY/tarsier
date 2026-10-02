# 厂商私有通道

[English](private-channels.en.md) · **简体中文** · [返回 README](../README.md)

## 问题

新款 LG 显示器（例如 28MQ780）会**确认** VCP 0x60 的写入，但实际并不切换输入。它们只认从主机地址 `0x50` 发出的 VCP 0xF4（[ddcutil wiki](https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors)）。

Windows 的显示器 API（`SetVCPFeature` 之类）固定用 `0x51`，这个地址改不了。

## 办法

绕开显示器 API，借显卡驱动直接往 I²C 总线上发包。

| 显卡 | 原始 I²C 后端 |
| --- | --- |
| NVIDIA | NVAPI（`nvapi64.dll`） |
| Intel | IGCL（驱动自带的 `ControlLib.dll`）。DisplayPort / USB-C 走 I²C-over-AUX，HDMI 走 DDC 引脚 |
| AMD | 暂不支持 |

注意是看**驱动这台显示器的那块显卡**，不是插了线的那块。混合显卡笔记本上外接口常常接在核显上。

tarsier 会先尝试标准 VCP 0x60，写入后回读确认；如果显示器确认了却没真的切换，就自动回落到私有通道。也可以用 `input_protocol` 手工指定，跳过检测。

## UAC

Intel 驱动只允许管理员权限的进程写 I²C。tarsier 本身不以管理员身份运行，需要时用管理员身份临时启动一个只发这一条命令的子进程（`--raw-ddc-write`），所以**每次通过 Intel 私有通道切换输入都会弹一次 UAC**。

NVIDIA 的 NVAPI 没有这个限制，切换时不会弹窗。

## 输入值映射

MCCS 的输入值和 LG 私有的编号不是一套。内置映射表覆盖常见输入，`input_protocol` 里的 `values` 可以覆盖它：

```json
{ "input_protocol": { "kind": "lg", "values": { "16": 210 } } }
```

键是十进制 MCCS 输入值（16 = DisplayPort），值是 LG 的编号。其余取值见 [ddcutil wiki](https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors#theoretically-supported)。
