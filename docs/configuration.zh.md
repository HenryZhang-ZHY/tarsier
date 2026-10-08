# 配置参考

[English](configuration.md) · **简体中文** · [返回 README](../README.zh.md)

**设置 → 高级**里的「设置文件夹」打开 `%APPDATA%\tarsier\`（设置了 `TARSIER_HOME` 时打开它指向的目录，见[构建与运行](building.zh.md)）。其中的文件如果解析失败（手改出错，或由更新的版本写入），会被改名为 `*.json.broken`，tarsier 从默认值启动，原文件不会被覆盖：

| 文件 | 内容 |
| --- | --- |
| `config.json` | 设置 |
| `stats.json` | 每天的记录：工作段、休息提醒、第一次和最后一次输入、下机时间，以及继续使用时写下的理由 |
| `tarsier.log` | 日志 |

界面里能改的都会写回 `config.json`。下面这些也可以手工编辑；没有界面入口的字段已标出，那些改完需要重启。文件是 JSON，缺失的字段自动取默认值，写坏了会被忽略并记一条警告。

## 顶层字段

### `hotkeys`

快捷键，`global-hotkey` 语法，留空表示禁用。它们就是**设置 → 快捷键**里的四行，平时在这里点「录制」改即可 —— 改完立即生效，不用重启。

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

格式如 `"ctrl+alt+I"`、`"ctrl+shift+F12"`。一个组合键只做一件事：录制一个已经被别的行占用的组合键会把那一行清空，因为 Windows 会拒绝已经注册过的组合键 —— 包括被 tarsier 自己占用的。注册失败的原因显示在这些行的下面。

### `brightness_step`

快捷键每按一次调整的亮度步长，单位是百分比，默认 `10`。**没有界面入口**，改文件后重启生效。

### `developer_mode`

开发者模式，也可以在设置页打开。见[开发者模式](developer-mode.zh.md)。

### `language`

`"en"`（默认）或 `"zh"`，也可以在设置页切换。所有窗口和托盘菜单都用这个语言。英文是源语言，所以没有中文翻译的句子会直接显示英文，而不是显示成空白或占位符。

### `skin`

主窗口用哪套皮肤，也可以在设置页切换。

| 取值 | 外观 |
| --- | --- |
| `"native"`（默认） | 组件库原本的样子：系统字体、圆角、克制的灰色。也就是 tarsier 在还没有皮肤之前的样子。 |
| `"neo_brutalism"` | 暖纸色、黑墨线、实心描边、硬位移阴影、一种黄色强调色，以及内置的 Space Grotesk 字体。 |

皮肤不只是配色：画布、面板、字体、控件都归它管。它同时是**最顶层**的外观选择，因为一套皮肤可能只为某一种明暗模式而设计。新粗野主义按设计只有一种确定的浅色配色，所以选中它会把窗口固定在浅色，设置页会直接说明这一点，而不是给你一个点了没反应的切换器。

### `theme`

`"light"`、`"dark"` 或 `"system"`（默认），也可以在设置页切换。支持浅色深色两种模式的皮肤都会遵循它。只提供一种模式的皮肤（比如新粗野主义）不管这里写什么都会画那一种，但取值会保留下来——切回支持它的皮肤时，选择会原样恢复。取值是 `"system"` 时，它跟随 Windows 的浅色 / 深色设置实时变化。

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

`respect_fullscreen` 为真时，检测到全屏游戏、视频或 PPT 演示会推迟提醒。规则见[休息提醒、下机时间与这一周](breaks.zh.md)。

### `evening`

```json
{
  "evening": {
    "enabled": false,
    "cutoff": "22:00"
  }
}
```

下机时间，也可以在**设置 → 休息**里改。`cutoff` 是 `"12:00"` 到 `"04:59"` 之间的时间：一天从早上 5 点重新开始，早上的下机时间会把整个白天都盖住。写成别的值会退回 `"22:00"` 并在日志里记一条警告，不会连累文件里的其他设置。规则见[休息提醒、下机时间与这一周](breaks.zh.md#下机时间)。

## `stats.json`

按 `YYYY-MM-DD` 记录每一天，一天从早上 5 点到第二天早上 5 点。文件带 `"version": 2`。更早版本的文件（没有版本号，按午夜分日，跳过和推迟只记次数）在第一次启动时会先复制为 `stats.v1.json`，再转换：工作段按结束时间重新归到对应的日子；那几个次数没有时间，无法转换，直接丢弃。

## 每台显示器的设置：`monitors.<id>`

键是显示器 id（在「显示器」页的诊断信息里能看到）。所有字段都可以不填。

### `endpoints`

这台显示器上接着的电脑，按顺序写端口（十进制 VCP 值）：

```json
{ "monitors": { "DEL41A3": { "endpoints": [16, 18, 17] } } }
```

15 = DisplayPort 1，17 = HDMI 1。物理接口对应关系以显示器自己的上报为准，不同型号不一样。

**顺序有意义**：三台以上时，顺序就是快切面板里数字键的编号，也是主窗口里行首的序号。

两台时快捷键是**盲翻**：按下时读一次显示器的当前输入，切到另一个。所以两个端点的先后无所谓。**读不出来的时候它不猜** —— 显示器没上报、或者根本没在这两个口上，就直接弹出快切面板让你选，而不是闷声什么都不做。三台起快捷键**呼出快切面板**，按数字直达 —— 不做循环，因为多数显示器被切走后就不再响应本机的 DDC/CI，第二跳很可能发不出去。

> 旧版本用 `toggle: [15, 17]` 描述同一件事。它会在加载时自动升级成 `endpoints`，`input_names` 原样保留。

### `local_input`

**这台电脑**插在哪个口。只影响界面上的「本机」标记 —— 界面里所有按钮说的都是「切到哪儿」，不表示「现在在哪儿」，所以切换逻辑完全不依赖它。

```json
{ "local_input": 16 }
```

不填也不影响用，只是不会标出哪一行是你正坐着的这台。

> 界面刻意不显示「显示器现在在哪一台」：那个值只能靠定时去读显示器才知道，而另一台电脑随时可能把它改掉，与其显示一个可能过期的状态，不如只提供「切到 X」的按钮 —— 按钮永远不会说谎。

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

原理见[厂商私有通道](private-channels.zh.md)。

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
  "language": "en",
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
