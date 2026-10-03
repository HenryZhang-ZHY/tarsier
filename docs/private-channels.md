# Vendor-private channels

**English** · [简体中文](private-channels.zh.md) · [Back to README](../README.md)

## The problem

Recent LG monitors (the 28MQ780, for instance) **acknowledge** a write to VCP 0x60 but don't actually switch input. They only accept VCP 0xF4 sent from host address `0x50` ([ddcutil wiki](https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors)).

Windows' monitor API (`SetVCPFeature` and friends) is hardcoded to `0x51`, and that address can't be changed.

## The workaround

Skip the monitor API and send the packet straight onto the I²C bus through the GPU driver.

| GPU | Raw I²C backend |
| --- | --- |
| NVIDIA | NVAPI (`nvapi64.dll`) |
| Intel | IGCL (the driver's own `ControlLib.dll`). DisplayPort / USB-C go over I²C-over-AUX, HDMI over the DDC pins |
| AMD | Not supported yet |

Note that what matters is the GPU **driving that monitor**, not the one the cable is plugged into. On hybrid-graphics laptops the external port often hangs off the integrated GPU.

tarsier tries the standard VCP 0x60 first and reads back to confirm. If the monitor acknowledged the write without actually switching, it falls back to the private channel automatically. You can also force the choice with `input_protocol` to skip detection.

## UAC

The Intel driver only lets elevated processes write to I²C. tarsier itself does not run as administrator; it launches a short-lived elevated child process (`--raw-ddc-write`) that sends that single command, so **every input switch over the Intel private channel raises a UAC prompt**.

NVIDIA's NVAPI has no such restriction, so those switches are silent.

## Input value mapping

MCCS input values and LG's private numbering are two different scales. A built-in table covers the common inputs; `values` inside `input_protocol` overrides it:

```json
{ "input_protocol": { "kind": "lg", "values": { "16": 210 } } }
```

Keys are decimal MCCS input values (16 = DisplayPort), values are LG's numbers. The rest are listed on the [ddcutil wiki](https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors#theoretically-supported).
