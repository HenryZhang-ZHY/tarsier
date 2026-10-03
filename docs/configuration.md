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

### `endpoints`

这台显示器上接着的电脑，按顺序写端口（十进制 VCP 值）：

```json
{ "monitors": { "DEL41A3": { "endpoints": [16, 18, 17] } } }
```

15 = DisplayPort 1，17 = HDMI 1。物理接口对应关系以显示器自己的上报为准，不同型号不一样。

**顺序有意义**：三台以上时，顺序就是快切面板里数字键的编号，也是主窗口里行首的序号。

两台时快捷键是**盲翻**（读显示器当前输入，切到另一个），所以两个端点的先后无所谓；三台起快捷键**呼出快切面板**，按数字直达 —— 不做循环，因为多数显示器被切走后就不再响应本机的 DDC/CI，第二跳很可能发不出去。

> 旧版本用 `toggle: [15, 17]` 描述同一件事。它会在加载时自动升级成 `endpoints`，`input_names` 原样保留。

### `local_input`

**这台电脑**插在哪个口。只影响界面显示（哪一行标「本机」），不参与切换 —— 切换永远读显示器自己上报的当前输入。

```json
{ "local_input": 16 }
```

不填也不影响用，只是主窗口和快切面板不会标出「你在哪一边」。

### `input_names`

给每台电脑起名字，键是十进制 VCP 值：

```json
{ "input_names": { "15": "台式机", "17": "笔记本" } }
```

名字会出现在主窗口、托盘菜单和快切面板里 —— 这是唯一能让你一眼认出「谁是谁」的东西，也是取代旧版 A / B 两个字母的地方。「口 → 名字」是绝对映射（DisplayPort 2 在你哪台电脑上都指同一台机器），所以同一套名字在每台机器上都成立。

主窗口里直接改就行，不用手写 JSON。

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
      "endpoints": [16, 18, 17],
      "local_input": 16,
      "input_names": { "16": "笔记本", "17": "台式机", "18": "游戏机" },
      "extra_inputs": [27]
    }
  }
}
```
