# Material Symbols Outlined（图标字体）

任务卡片图标字体（开始 / 完成 / 删除 / 编辑 / 项目 / 类型 / 截止 / 耗时）。

- **来源**：Google Material Symbols（OFL 开源许可）
  - 原始可变字体：`https://github.com/google/material-design-icons/raw/master/variablefont/MaterialSymbolsOutlined%5BFILL%2CGRAD%2Copsz%2Cwght%5D.ttf`（约 10.6MB）
  - 码位表：同目录 `...codepoints`（Material Symbols 2.962）
- **生成**（子集化，10.6MB → 2.3KB）：
  1. `instantiateVariableFont` 固定可变轴：`wght=400, opsz=24, FILL=0, GRAD=0`
  2. `pyftsubset --unicodes="U+E037,U+E668,U+E92E,U+F097,U+E2C7,U+F05B,U+EFD6,U+E425" --no-hinting`
- **校验**：家族名 `Material Symbols Outlined`（nameID 1，与 `tokens.rs::ICON_FONT` 的 `Font::with_name` 一致）；magic bytes `00010000`（TrueType）；cmap 含下表全部码位。

| 用途 | 字形 | 码位 |
| ---- | ---- | ---- |
| 开始 | play_arrow | U+E037 |
| 完成 | check | U+E668 |
| 删除 | delete | U+E92E |
| 编辑 | edit | U+F097 |
| 项目 | folder | U+E2C7 |
| 类型 | sell | U+F05B |
| 截止时间 | schedule | U+EFD6 |
| 耗时 | timer | U+E425 |

加载方式：`src/main.rs` boot 经 `iced::font::load(include_bytes!(...))` 编译期嵌入。
