//! 主题调色板与语义色：两套自定义 `Palette`（浅色「晴空」/ 深色「夜航」）
//! + 派生扩展色缓存 + 语义色双板（SemColors）。
//!
//! main.rs 的主题装配（`Theme::custom` 选板）、view 层样式与语义色取板共用
//! `App::is_dark()` 同一判定；对比度由本模块测试锁定（WCAG 2.x，正文 ≥ 4.5:1）。

use std::sync::LazyLock;

use iced::Color;
use iced::color;

use crate::model::App;

/// 浅色主题「晴空」：冷调近白底 + 靛蓝主色（现代生产力工具风）。
/// 全部关键文字配对对比度 ≥ 4.5:1（tests::palette_contrast 按真实派生底色断言）。
pub(crate) const LIGHT_PALETTE: iced::theme::Palette = iced::theme::Palette {
    background: color!(0xF7F8FA),
    text: color!(0x1F2328),
    primary: color!(0x1D4ED8),
    success: color!(0x166534),
    warning: color!(0x92400E),
    danger: color!(0x991B1B),
};

/// 深色主题「夜航」：Tokyo Night 蓝紫底 + 提亮主色（去饱和防眩光，分层提亮）。
pub(crate) const DARK_PALETTE: iced::theme::Palette = iced::theme::Palette {
    background: color!(0x1A1B26),
    text: color!(0xD7D9E3),
    primary: color!(0x8FB2FF),
    success: color!(0x9ECE6A),
    warning: color!(0xE0AF68),
    danger: color!(0xF7768E),
};

/// 两套调色板的派生扩展色静态缓存：`extended_palette()` 的等价物，
/// 供无 `&Theme` 参数的位置取派生文字色（如选中芯片文字 `primary.weak.text`）。
static LIGHT_EXTENDED: LazyLock<iced::theme::palette::Extended> =
    LazyLock::new(|| iced::theme::palette::Extended::generate(LIGHT_PALETTE));
static DARK_EXTENDED: LazyLock<iced::theme::palette::Extended> =
    LazyLock::new(|| iced::theme::palette::Extended::generate(DARK_PALETTE));

/// 当前主题的派生扩展色（与 main.rs 的主题装配共用 `App::is_dark` 判定）。
pub(crate) fn extended(app: &App) -> &'static iced::theme::palette::Extended {
    if app.is_dark() {
        &DARK_EXTENDED
    } else {
        &LIGHT_EXTENDED
    }
}

/// 语义色（随深浅主题切换的双板）：状态徽章 / 错误 / 次要文字等固定色不能依赖
/// `extended_palette` 派生，按主题显式定义；对比度均 ≥ 4.5:1（tests 锁定）。
#[derive(Clone, Copy)]
pub(crate) struct SemColors {
    /// 次要文字（标签 / 提示 / 时间元信息 / 无项目）
    pub(crate) muted: Color,
    /// 进行中高亮（状态徽章 / 实时耗时）
    pub(crate) blue: Color,
    /// 已完成（徽章 / 总耗时）
    pub(crate) done: Color,
    /// 中优先级（徽章 / 圆点）
    pub(crate) accent: Color,
    /// 错误 / 逾期 / 高优先级
    pub(crate) error: Color,
}

/// 浅色主题语义色。
const LIGHT_SEM: SemColors = SemColors {
    muted: color!(0x4B5563),
    blue: color!(0x1D4ED8),
    done: color!(0x166534),
    accent: color!(0x92400E),
    error: color!(0x991B1B),
};

/// 深色主题语义色（深色下用提亮的同族色，保证弱底可读）。
const DARK_SEM: SemColors = SemColors {
    muted: color!(0xA6ADB8),
    blue: color!(0x8FB2FF),
    done: color!(0x9ECE6A),
    accent: color!(0xE0AF68),
    error: color!(0xF7768E),
};

/// 按是否暗色取语义色板。
pub(crate) const fn sem_colors(dark: bool) -> SemColors {
    if dark { DARK_SEM } else { LIGHT_SEM }
}

/// 当前主题的语义色（与 main.rs 的主题装配共用 `App::is_dark` 判定）。
pub(crate) fn sem(app: &App) -> SemColors {
    sem_colors(app.is_dark())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::theme::palette::Extended;

    /// WCAG 2.x 相对亮度（sRGB → 线性 → 加权和）。
    fn relative_luminance(color: Color) -> f32 {
        let linear = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    /// WCAG 2.x 对比度（1.0 ~ 21.0）。
    fn contrast_ratio(a: Color, b: Color) -> f32 {
        let (l1, l2) = (relative_luminance(a), relative_luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// 两套调色板的关键文字配对对比度（含 iced 真实派生底色，`Extended::generate`）：
    /// 正文 ≥ 7.0；六色 / 语义色 vs 主背景与弱底 ≥ 4.5（WCAG AA）；下拉 handle（强底）按 UI 元素 ≥ 3.0。
    #[test]
    fn palette_and_sem_colors_contrast() {
        for (name, palette, sem) in [
            ("浅色", LIGHT_PALETTE, LIGHT_SEM),
            ("深色", DARK_PALETTE, DARK_SEM),
        ] {
            // 正文对比（AA 之上，正文 7:1 更舒适）
            assert!(
                contrast_ratio(palette.text, palette.background) >= 7.0,
                "{name} 正文对比度不足: {}",
                contrast_ratio(palette.text, palette.background)
            );
            // Palette 六色 vs 主背景
            for (label, color) in [
                ("primary", palette.primary),
                ("success", palette.success),
                ("warning", palette.warning),
                ("danger", palette.danger),
            ] {
                assert!(
                    contrast_ratio(color, palette.background) >= 4.5,
                    "{name} {label} 在主背景上对比度不足: {}",
                    contrast_ratio(color, palette.background)
                );
            }
            // 真实派生底色（iced `Extended::generate`，非估算）
            let ext = Extended::generate(palette);
            let weak = ext.background.weak.color;
            let strong = ext.background.strong.color;
            // 语义色 vs 主背景与卡片弱底
            for (label, color) in [
                ("muted", sem.muted),
                ("blue", sem.blue),
                ("done", sem.done),
                ("accent", sem.accent),
                ("error", sem.error),
            ] {
                assert!(
                    contrast_ratio(color, palette.background) >= 4.5,
                    "{name} {label} 在主背景上对比度不足: {}",
                    contrast_ratio(color, palette.background)
                );
                assert!(
                    contrast_ratio(color, weak) >= 4.5,
                    "{name} {label} 在弱底上对比度不足: {}",
                    contrast_ratio(color, weak)
                );
            }
            // 选中芯片文字：primary.weak 底上的派生可读配对色（iced readable 兜底）
            assert!(
                contrast_ratio(ext.primary.weak.text, ext.primary.weak.color) >= 4.5,
                "{name} 选中芯片文字对比度不足: {}",
                contrast_ratio(ext.primary.weak.text, ext.primary.weak.color)
            );
            // 未选中芯片文字：主文字叠弱底（project_chip_style 未选中态实际渲染配对）
            assert!(
                contrast_ratio(ext.background.base.text, weak) >= 4.5,
                "{name} 未选中芯片文字对比度不足: {}",
                contrast_ratio(ext.background.base.text, weak)
            );
            // 统计胶囊链接文字（primary 色，弱底）
            assert!(
                contrast_ratio(palette.primary, weak) >= 4.5,
                "{name} 链接文字对比度不足: {}",
                contrast_ratio(palette.primary, weak)
            );
            // 错误横幅文字（danger 色，弱底）
            assert!(
                contrast_ratio(palette.danger, weak) >= 4.5,
                "{name} 错误横幅文字对比度不足: {}",
                contrast_ratio(palette.danger, weak)
            );
            // 下拉 handle（muted，强底）：UI 元素按 3:1
            assert!(
                contrast_ratio(sem.muted, strong) >= 3.0,
                "{name} 下拉 handle 对比度不足: {}",
                contrast_ratio(sem.muted, strong)
            );
        }
    }

    /// 两套调色板的明暗语义与语义色取板正确。
    #[test]
    fn palette_dark_semantics() {
        assert!(
            !Extended::generate(LIGHT_PALETTE).is_dark,
            "浅色板被判定为暗色"
        );
        assert!(
            Extended::generate(DARK_PALETTE).is_dark,
            "深色板被判定为亮色"
        );
        assert_eq!(sem_colors(false).muted, LIGHT_SEM.muted);
        assert_eq!(sem_colors(true).muted, DARK_SEM.muted);
    }
}
