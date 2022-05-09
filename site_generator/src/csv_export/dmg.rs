use crate::legacy::console::{
    LegacyDmgJackBoard, LegacyDmgLcdBoard, LegacyDmgMainboard, LegacyDmgMetadata,
    LegacyDmgPowerBoard, LegacyLcdPanel,
};

use super::{calendar, calendar_short, chip, Builder, Field, ToCsv};

impl ToCsv for LegacyDmgMetadata {
    fn csv_builder() -> Builder<Self> {
        Builder::<Self>::new()
            .add("color", |m| (&m.color).csv())
            .add("calendar_short", |m| {
                calendar_short(m.year, m.month, None).csv()
            })
            .add("calendar", |m| calendar(m.year, m.month, None).csv())
            .add("year", |m| m.year.csv())
            .add("month", |m| m.month.csv())
            .nest(
                "mainboard",
                |m| Some(&m.mainboard),
                || {
                    Builder::<LegacyDmgMainboard>::new()
                        .add("type", |m| (&m.kind).csv())
                        .add("extra_label", |m| (&m.extra_label).csv())
                        .add("stamp", |m| (&m.stamp).csv())
                        .add("circled_letters", |m| (&m.circled_letters).csv())
                },
            )
            .nest("cpu", |m| m.mainboard.cpu.as_ref(), chip)
            .nest("work_ram", |m| m.mainboard.work_ram.as_ref(), chip)
            .nest("video_ram", |m| m.mainboard.video_ram.as_ref(), chip)
            .nest("amplifier", |m| m.mainboard.amplifier.as_ref(), chip)
            .nest("crystal", |m| m.mainboard.crystal.as_ref(), chip)
            .nest(
                "lcd_board",
                |m| m.lcd_board.as_ref(),
                || {
                    Builder::<LegacyDmgLcdBoard>::new()
                        .add("type", |b| (&b.kind).csv())
                        .add("circled_letters", |b| (&b.circled_letters).csv())
                        .add("stamp", |b| (&b.stamp).csv())
                        .add("calendar_short", |b| {
                            calendar_short(b.year, b.month, None).csv()
                        })
                        .add("calendar", |b| calendar(b.year, b.month, None).csv())
                        .add("year", |b| b.year.csv())
                        .add("month", |b| b.month.csv())
                },
            )
            .nest(
                "lcd_panel",
                |m| m.lcd_board.as_ref().and_then(|b| b.lcd_panel.as_ref()),
                || {
                    Builder::<LegacyLcdPanel>::new()
                        .add("label", |p| (&p.label).csv())
                        .add("calendar_short", |p| {
                            calendar_short(p.year, p.month, None).csv()
                        })
                        .add("calendar", |p| calendar(p.year, p.month, None).csv())
                },
            )
            .nest(
                "column_driver",
                |m| {
                    m.lcd_board
                        .as_ref()
                        .and_then(|b| b.lcd_panel.as_ref().and_then(|p| p.column_driver.as_ref()))
                },
                chip,
            )
            .nest(
                "row_driver",
                |m| {
                    m.lcd_board
                        .as_ref()
                        .and_then(|b| b.lcd_panel.as_ref().and_then(|p| p.row_driver.as_ref()))
                },
                chip,
            )
            .nest(
                "regulator",
                |m| m.lcd_board.as_ref().and_then(|b| b.regulator.as_ref()),
                chip,
            )
            .nest(
                "power_board",
                |m| m.power_board.as_ref(),
                || {
                    Builder::<LegacyDmgPowerBoard>::new()
                        .add("type", |b| (&b.kind).csv())
                        .add("label", |b| (&b.label).csv())
                        .add("calendar_short", |b| {
                            calendar_short(b.year, b.month, None).csv()
                        })
                        .add("calendar", |b| calendar(b.year, b.month, None).csv())
                        .add("year", |b| b.year.csv())
                        .add("month", |b| b.month.csv())
                },
            )
            .nest(
                "jack_board",
                |m| m.jack_board.as_ref(),
                || {
                    Builder::<LegacyDmgJackBoard>::new()
                        .add("type", |b| (&b.kind).csv())
                        .add("extra_label", |b| (&b.extra_label).csv())
                },
            )
    }
}
