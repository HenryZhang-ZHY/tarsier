# 配置参考

[English](configuration.en.md) · **简体中文** · [返回 README](../README.md)

设置页里的「打开配置文件夹」对应 `%APPDATA%\tarsier\`：

| 文件 | 内容 |
| --- | --- |
| `config.json` | 设置 |
| `stats.json` | 每天的工作段记录 |
| `tarsier.log` | 日志 |

设置页里能改的都会写回 `config.json`。下面这些**只能手工编辑**，改完重启生效。文件是 JSON，缺失的字段自动取默认值，写坏了会被忽略并记一条警告。

## 顶层字段

### `hotkeys`

快捷键，`global-hotkey` 语法，留空表示禁用。

```json
{
  "hotkeys": {
    "toggle_input": "ctrl+alt+I",
    "brightness_up": "ctrl+alt+PageUp",
    "brightness_down": "ctrl+alt+PageDown",
    "break_now": ""
  }
}
```

格式如 `"ctrl+alt+I"`、`"ctrl+shift+F12"`。注册失败会在设置页显示原因。

### `brightness_step`

快捷键每按一次调整的亮度步长，单位是百分比，默认 `10`。

### `developer_mode`

开发者模式，也可以在设置页打开。见[开发者模式](developer-mode.md)。

### `breaks`

```json
{
  "breaks": {
    "enabled": true,
    "work_minutes": 50,
    "break_minutes": 5,
    "snooze_minutes": 5,
    "respect_fullscreen": true
  }
}
```

`respect_fullscreen` 为真时，检测到全屏游戏、视频或 PPT 演示会推迟提醒。规则见[休息提醒与评分](breaks.md)。

## 每台显示器的设置：`monitors.<id>`

键是显示器 id（在「显示器」页的诊断信息里能看到）。所有字段都可以不填。

### `toggle`

快捷键要来回切换的那两个输入，十进制 VCP 值：

```json
{ "monitors": { "DEL41A3": { "toggle": [15, 17] } } }
```

15 = DisplayPort 1，17 = HDMI 1。物理接口对应关系以显示器自己的上报为准，不同型号不一样。

### `input_names`

给输入起名字，键是十进制 VCP 值：

```json
{ "input_names": { "15": "台式机", "17": "笔记本" } }
```

名字会出现在「显示器」页和托盘菜单里。

### `extra_inputs`

显示器没有上报的输入值。有些显示器接在 USB-C 上却不报 27（0x1B），补进来就能选：

```json
{ "extra_inputs": [27] }
```

### `input_protocol`

强制指定切换输入的方式，一般不用填，tarsier 会自己检测。

- `{"kind": "mccs"}`：标准 VCP 0x60。
- `{"kind": "lg", "values": {"16": 210}}`：LG 私有通道。`values` 把十进制 MCCS 输入值映射到 LG 的编号，会覆盖内置表。

原理见[厂商私有通道](private-channels.md)。

## 完整例子

```json
{
  "hotkeys": {
    "toggle_input": "ctrl+alt+I",
    "brightness_up": "ctrl+alt+PageUp",
    "brightness_down": "ctrl+alt+PageDown",
    "break_now": ""
  },
  "brightness_step": 10,
  "developer_mode": false,
  "breaks": {
    "enabled": true,
    "work_minutes": 50,
    "break_minutes": 5,
    "snooze_minutes": 5,
    "respect_fullscreen": true
  },
  "monitors": {
    "DEL41A3": {
      "toggle": [15, 17],
      "input_names": { "15": "台式机", "17": "笔记本" },
      "extra_inputs": [27]
    }
  }
}
```
