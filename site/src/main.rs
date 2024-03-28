// SPDX-FileCopyrightText: 2017-2023 Joonas Javanainen <joonas.javanainen@gmail.com>
//
// SPDX-License-Identifier: MIT

use anyhow::Error;
use csv_export::{write_submission_csv, ToCsv};
use filetime::{set_file_mtime, FileTime};
use gbhwdb_backend::{
    config::cartridge::*,
    input::cartridge::*,
    parser::{self, LabelParser},
    Console,
};
use glob::glob;
use image::{imageops::FilterType, ImageOutputFormat};
use legacy::LegacyPhotos;
use log::{debug, info, warn, LevelFilter};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, create_dir_all, File, Metadata},
    io::{BufWriter, Write},
    path::Path,
};
use walkdir::{DirEntry, WalkDir};

use crate::legacy::part::*;
use crate::legacy::*;
use site::{build_site, SubmissionCounts};

mod css;
mod csv_export;
mod legacy;
mod site;
mod template;

fn is_metadata_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file() && entry.file_name() == "metadata.json"
}

fn get_photo(root: &Path, name: &str) -> Option<LegacyPhoto> {
    if root.join(name).exists() {
        Some(LegacyPhoto {
            path: root
                .canonicalize()
                .unwrap()
                .join(name)
                .display()
                .to_string(),
            name: name.to_owned(),
        })
    } else {
        None
    }
}

#[derive(Default)]
pub struct SiteData {
    cfgs: BTreeMap<String, GameConfig>,
    cartridges: Vec<LegacyCartridgeSubmission>,
    dmg: Vec<LegacyDmgSubmission>,
    sgb: Vec<LegacySgbSubmission>,
    mgb: Vec<LegacyMgbSubmission>,
    mgl: Vec<LegacyMglSubmission>,
    sgb2: Vec<LegacySgb2Submission>,
    cgb: Vec<LegacyCgbSubmission>,
    agb: Vec<LegacyAgbSubmission>,
    ags: Vec<LegacyAgsSubmission>,
    gbs: Vec<LegacyGbsSubmission>,
    oxy: Vec<LegacyOxySubmission>,
}

impl SiteData {
    pub fn counts(&self) -> SubmissionCounts {
        SubmissionCounts {
            cartridges: self.cartridges.len() as u32,
            consoles: HashMap::from([
                (Console::Dmg, self.dmg.len() as u32),
                (Console::Sgb, self.sgb.len() as u32),
                (Console::Mgb, self.mgb.len() as u32),
                (Console::Mgl, self.mgl.len() as u32),
                (Console::Sgb2, self.sgb2.len() as u32),
                (Console::Cgb, self.cgb.len() as u32),
                (Console::Agb, self.agb.len() as u32),
                (Console::Ags, self.ags.len() as u32),
                (Console::Gbs, self.gbs.len() as u32),
                (Console::Oxy, self.oxy.len() as u32),
            ]),
        }
    }
}

fn build_css() -> Result<(), Error> {
    create_dir_all("build/static")?;

    let mut css = fs::read_to_string("third-party/normalize.css")?;
    css.push_str(&css::read_sass("site/src/gbhwdb.scss")?);

    let css = css::minify(&css)?;
    fs::write("build/static/gbhwdb.css", css.as_bytes())?;
    Ok(())
}

fn is_outdated(ref_meta: &Metadata, path: &Path) -> bool {
    match path.metadata() {
        Ok(meta) if meta.modified().ok() == ref_meta.modified().ok() => false,
        _ => true,
    }
}

fn convert_photo(
    input: impl AsRef<Path>,
    target: impl AsRef<Path>,
    width: u32,
) -> Result<(), Error> {
    let img = image::open(input.as_ref())?.resize(width, u32::MAX, FilterType::Lanczos3);
    let mut w = BufWriter::new(File::create(&target)?);
    img.write_to(&mut w, ImageOutputFormat::Jpeg(80))?;
    w.flush()?;
    Ok(())
}

fn process_photos<M, P>(submissions: &[LegacySubmission<M, P>]) -> Result<(), Error>
where
    M: Sync + Send,
    P: Sync + Send + LegacyPhotos,
{
    submissions
        .par_iter()
        .map(|submission| {
            let target_dir = Path::new("build/static").join(&submission.code);
            fs::create_dir_all(&target_dir)?;
            if let Some(front) = submission.photos.front() {
                let ref_meta = Path::new(&front.path).metadata()?;
                for width in [80, 50] {
                    let target = target_dir.join(format!(
                        "{slug}_thumbnail_{width}.jpg",
                        slug = submission.slug
                    ));
                    if is_outdated(&ref_meta, &target) {
                        convert_photo(&front.path, &target, width)?;
                        set_file_mtime(&target, FileTime::from_last_modification_time(&ref_meta))?;
                        debug!("Wrote thumbnail {target}", target = target.display());
                    }
                }
            }
            for photo in submission.photos.photos() {
                let ref_meta = Path::new(&photo.path).metadata()?;
                let target = target_dir.join(format!(
                    "{slug}_{name}",
                    slug = submission.slug,
                    name = photo.name
                ));
                if is_outdated(&ref_meta, &target) {
                    fs::copy(&photo.path, &target)?;
                    set_file_mtime(&target, FileTime::from_last_modification_time(&ref_meta))?;
                    debug!("Copied photo {target}", target = target.display());
                }
            }
            Ok(())
        })
        .collect::<Result<(), Error>>()
}

fn main() -> Result<(), Error> {
    let _ = TermLogger::init(
        LevelFilter::Info,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );

    let mut data = SiteData::default();
    create_dir_all("build/static/export/consoles")?;

    info!("Processing submissions");

    let cfgs = gbhwdb_backend::config::cartridge::load_cfgs("config/games.json")?;

    data.cartridges = process_cartridge_submissions(&cfgs)?;
    data.dmg = process_dmg_submissions()?;
    data.sgb = process_sgb_submissions()?;
    data.mgb = process_mgb_submissions()?;
    data.mgl = process_mgl_submissions()?;
    data.sgb2 = process_sgb2_submissions()?;
    data.cgb = process_cgb_submissions()?;
    data.agb = process_agb_submissions()?;
    data.ags = process_ags_submissions()?;
    data.gbs = process_gbs_submissions()?;
    data.oxy = process_oxy_submissions()?;
    data.cfgs = cfgs;

    info!("Processing photos");

    process_photos(&data.cartridges)?;
    process_photos(&data.dmg)?;
    process_photos(&data.sgb)?;
    process_photos(&data.mgb)?;
    process_photos(&data.mgl)?;
    process_photos(&data.sgb2)?;
    process_photos(&data.cgb)?;
    process_photos(&data.agb)?;
    process_photos(&data.ags)?;
    process_photos(&data.gbs)?;
    process_photos(&data.oxy)?;

    info!("Generating site");

    let mut site = build_site();
    site.generate_all(&data, "build")?;
    build_css()?;
    copy_static_files()?;

    info!("Site generation finished");
    Ok(())
}

fn write_console_submission_csv<M, P>(
    kind: &'static str,
    submissions: &[LegacySubmission<M, P>],
) -> Result<(), Error>
where
    M: ToCsv,
{
    let csv = BufWriter::new(File::create(format!(
        "build/static/export/consoles/{kind}.csv"
    ))?);
    write_submission_csv(csv, "https://gbhwdb.gekkio/consoles", submissions)
}

fn process_cartridge_submissions(
    cfgs: &BTreeMap<String, GameConfig>,
) -> Result<Vec<LegacyCartridgeSubmission>, Error> {
    use legacy::cartridge::*;
    let walker = WalkDir::new("data/cartridges").min_depth(3).max_depth(3);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let cartridge: Cartridge = serde_json::from_reader(file)?;
            assert_eq!(
                Some(cartridge.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            let cfg = cfgs.get(&cartridge.code).unwrap();

            let layout = BoardLayout::from_label(&cartridge.board.label).unwrap_or_else(|| {
                panic!(
                    "Failed to find board layout for board {}",
                    cartridge.board.label
                )
            });
            assert!(cfg.layouts.contains(&layout));

            if let Some(year) = cartridge.board.year {
                assert!(year >= 1989 && year < 2010);
            }

            if let Some(sha256) = cartridge.dump.as_ref().map(|dump| dump.sha256) {
                match cfg.sha256 {
                    None => warn!(
                        "Submission has SHA256 but config doesn't: {}",
                        cartridge.code
                    ),
                    Some(cfg_sha) if cfg_sha == sha256 => (),
                    _ => panic!("SHA256 mismatch: {}", cartridge.code),
                }
            }

            let board = LegacyBoard::new(cartridge.board, layout);
            let metadata = LegacyMetadata {
                cfg: cfg.clone(),
                code: cartridge.shell.code,
                stamp: cartridge.shell.stamp,
                board,
                dump: cartridge.dump,
            };
            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.pcb_front = get_photo(root, "02_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "03_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: cartridge.code,
                title: format!("Entry #{}", cartridge.index),
                slug: cartridge.slug,
                sort_group: None,
                contributor: cartridge.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.code.clone(), submission.slug.clone()));
    let csv = BufWriter::new(File::create("build/static/export/cartridges.csv")?);
    write_submission_csv(csv, "https://gbhwdb.gekkio/cartridges", &submissions)?;
    Ok(submissions)
}

fn process_dmg_submissions() -> Result<Vec<LegacyDmgSubmission>, Error> {
    use gbhwdb_backend::input::dmg::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/DMG").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: DmgConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let cpu = console.mainboard.u1.as_ref().map(|part| {
                boxed_parser(parser::gen1_soc::gen1_soc())(None, part)
                    .unwrap()
                    .unwrap_or_else(|| LegacyPart {
                        kind: Some("blob".to_string()),
                        ..LegacyPart::default()
                    })
            });
            let year_hint = cpu.as_ref().map(|cpu| cpu.date_code.year.unwrap_or(1996));

            let work_ram = console.mainboard.u2.as_ref().map(|part| {
                boxed_parser(parser::ram::ram())(year_hint, part)
                    .unwrap()
                    .unwrap_or_else(|| LegacyPart {
                        kind: Some("blob".to_string()),
                        ..LegacyPart::default()
                    })
            });
            let video_ram = console.mainboard.u3.as_ref().map(|part| {
                boxed_parser(parser::ram::ram())(year_hint, part)
                    .unwrap()
                    .unwrap_or_else(|| LegacyPart {
                        kind: Some("blob".to_string()),
                        ..LegacyPart::default()
                    })
            });
            let amplifier = console.mainboard.u4.as_ref().map(|part| {
                boxed_parser(parser::dmg_amp::dmg_amp())(year_hint, part)
                    .unwrap()
                    .unwrap_or_else(|| LegacyPart {
                        kind: Some("blob".to_string()),
                        ..LegacyPart::default()
                    })
            });
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_4mihz::crystal_4mihz(),
            );

            let mainboard = LegacyDmgMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                extra_label: console.mainboard.extra_label.clone(),
                stamp: console.mainboard.stamp.clone(),
                cpu,
                work_ram,
                video_ram,
                amplifier,
                crystal,
            };

            let lcd_board = console.lcd_board.as_ref().map(|board| {
                let regulator = map_legacy_part(year_hint, &board.chip, parser::dmg_reg::dmg_reg());
                let lcd_panel = board
                    .screen
                    .as_ref()
                    .and_then(|screen| to_legacy_lcd_panel(year_hint, screen));

                LegacyDmgLcdBoard {
                    kind: board.label.clone(),
                    circled_letters: board.circled_letters.clone(),
                    stamp: board.stamp.clone(),
                    year: board.year,
                    month: board.month,
                    lcd_panel,
                    regulator,
                }
            });

            let power_board = console
                .power_board
                .as_ref()
                .map(|board| LegacyDmgPowerBoard {
                    kind: board.kind.clone(),
                    label: (if board.kind == "D" {
                        "DC CONV2 DMG"
                    } else {
                        "DC CONV DMG"
                    })
                    .to_owned(),
                    year: board.year,
                    month: board.month,
                });

            let jack_board = console.jack_board.as_ref().map(|board| LegacyDmgJackBoard {
                kind: board.kind.clone(),
                extra_label: board.extra_label.clone(),
            });

            let mainboard_stamp = console
                .mainboard
                .stamp
                .as_ref()
                .filter(|_| !console.mainboard.outlier)
                .map(|stamp| {
                    gbhwdb_backend::parser::dmg_stamp::dmg_stamp()
                        .parse(&stamp)
                        .unwrap_or_else(|_| panic!("{}", stamp))
                });
            let lcd_board_stamp = console
                .lcd_board
                .as_ref()
                .and_then(|board| board.stamp.as_ref().filter(|_| !board.outlier))
                .map(|stamp| {
                    gbhwdb_backend::parser::dmg_stamp::dmg_stamp()
                        .parse(&stamp)
                        .unwrap_or_else(|_| panic!("{}", stamp))
                });
            let stamp = mainboard_stamp.or(lcd_board_stamp);

            let metadata = LegacyDmgMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                year: stamp
                    .as_ref()
                    .and_then(|stamp| to_legacy_year(year_hint, stamp.year)),
                month: stamp.as_ref().and_then(|stamp| stamp.month),
                mainboard,
                lcd_board,
                power_board,
                jack_board,
            };

            let has_outliers = console.shell.outlier
                || console.mainboard.outlier
                || console
                    .lcd_board
                    .as_ref()
                    .map(|board| {
                        board.outlier
                            || board
                                .screen
                                .as_ref()
                                .map(|screen| screen.outlier)
                                .unwrap_or(false)
                    })
                    .unwrap_or(false)
                || console
                    .power_board
                    .as_ref()
                    .map(|board| board.outlier)
                    .unwrap_or(false)
                || console
                    .jack_board
                    .as_ref()
                    .map(|board| board.outlier)
                    .unwrap_or(false);
            let mut photos = LegacyDmgPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.mainboard_front = get_photo(root, "03_mainboard_front.jpg");
            photos.mainboard_back = get_photo(root, "04_mainboard_back.jpg");
            photos.lcd_board_front = get_photo(root, "05_lcd_board_front.jpg");
            photos.lcd_board_back = get_photo(root, "06_lcd_board_back.jpg");
            photos.power_board_front = get_photo(root, "07_power_board_front.jpg");
            photos.power_board_back = get_photo(root, "08_power_board_back.jpg");
            photos.jack_board_front = get_photo(root, "09_jack_board_front.jpg");
            photos.jack_board_back = get_photo(root, "10_jack_board_back.jpg");
            submissions.push(LegacySubmission {
                code: "dmg".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: Some(
                    (match (console.shell.serial.is_some(), has_outliers) {
                        (true, false) => "A",
                        (false, false) => "B",
                        (true, true) => "C",
                        (false, true) => "D",
                    })
                    .to_owned(),
                ),
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("dmg", &submissions)?;
    Ok(submissions)
}

fn process_sgb_submissions() -> Result<Vec<LegacySgbSubmission>, Error> {
    use gbhwdb_backend::input::sgb::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/SGB").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: SgbConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );

            let year_hint = console.mainboard.year;
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::gen1_soc::gen1_soc(),
            );
            let icd2 = map_legacy_part(year_hint, &console.mainboard.u2, parser::icd2::icd2());
            let work_ram = map_legacy_part(year_hint, &console.mainboard.u3, parser::ram::ram());
            let video_ram = map_legacy_part(year_hint, &console.mainboard.u4, parser::ram::ram());
            let rom = map_legacy_part(year_hint, &console.mainboard.u5, parser::sgb_rom::sgb_rom());
            let cic = map_legacy_part(year_hint, &console.mainboard.u6, parser::cic::cic());
            let mainboard = LegacySgbMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                letter_at_top_right: console.mainboard.letter_at_top_right.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                icd2,
                work_ram,
                video_ram,
                rom,
                cic,
            };

            let metadata = LegacySgbMetadata {
                stamp: console.shell.stamp,
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "sgb".to_string(),
                title: format!("Unit #{}", console.index),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("sgb", &submissions)?;
    Ok(submissions)
}

fn process_mgb_submissions() -> Result<Vec<LegacyMgbSubmission>, Error> {
    use gbhwdb_backend::input::mgb::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/MGB").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: MgbConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year;
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::gen2_soc::gen2_soc(),
            );
            let work_ram = map_legacy_part(year_hint, &console.mainboard.u2, parser::ram::ram());
            let amplifier =
                map_legacy_part(year_hint, &console.mainboard.u3, parser::mgb_amp::mgb_amp());
            let regulator =
                map_legacy_part(year_hint, &console.mainboard.u4, parser::dmg_reg::dmg_reg());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_4mihz::crystal_4mihz(),
            );
            let mainboard = LegacyMgbMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                jun: console.mainboard.jun,
                cpu,
                work_ram,
                amplifier,
                regulator,
                crystal,
            };
            let lcd_panel = to_legacy_lcd_panel(year_hint, &console.screen);

            let stamp = console.mainboard.stamp.as_ref().map(|stamp| {
                gbhwdb_backend::parser::dmg_stamp::dmg_stamp()
                    .parse(&stamp)
                    .unwrap_or_else(|_| panic!("{}", stamp))
            });

            let metadata = LegacyMgbMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                year: stamp
                    .as_ref()
                    .and_then(|stamp| to_legacy_year(year_hint, stamp.year)),
                month: stamp.as_ref().and_then(|stamp| stamp.month),
                mainboard,
                lcd_panel,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "mgb".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("mgb", &submissions)?;
    Ok(submissions)
}

fn process_mgl_submissions() -> Result<Vec<LegacyMglSubmission>, Error> {
    use gbhwdb_backend::input::mgl::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/MGL").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: MglConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year;
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::gen2_soc::gen2_soc(),
            );
            let work_ram = map_legacy_part(year_hint, &console.mainboard.u2, parser::ram::ram());
            let amplifier =
                map_legacy_part(year_hint, &console.mainboard.u3, parser::mgb_amp::mgb_amp());
            let regulator =
                map_legacy_part(year_hint, &console.mainboard.u4, parser::dmg_reg::dmg_reg());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_4mihz::crystal_4mihz(),
            );
            let t1 = map_legacy_part(
                year_hint,
                &console.mainboard.t1,
                parser::mgl_transformer::mgl_transformer(),
            );
            let mainboard = LegacyMglMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                jun: console.mainboard.jun,
                cpu,
                work_ram,
                amplifier,
                regulator,
                crystal,
                t1,
            };
            let lcd_panel = to_legacy_lcd_panel(year_hint, &console.screen);

            let stamp = console.mainboard.stamp.as_ref().map(|stamp| {
                gbhwdb_backend::parser::cgb_stamp::cgb_stamp()
                    .parse(&stamp)
                    .unwrap_or_else(|_| panic!("{}", stamp))
            });

            let metadata = LegacyMglMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                year: stamp
                    .as_ref()
                    .and_then(|stamp| to_legacy_year(year_hint, stamp.year)),
                week: stamp.as_ref().and_then(|stamp| stamp.week),
                mainboard,
                lcd_panel,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "mgl".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("mgl", &submissions)?;
    Ok(submissions)
}

fn process_sgb2_submissions() -> Result<Vec<LegacySgb2Submission>, Error> {
    use gbhwdb_backend::input::sgb2::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/SGB2").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: Sgb2Console = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );

            let year_hint = console.mainboard.year;
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::gen2_soc::gen2_soc(),
            );
            let icd2 = map_legacy_part(year_hint, &console.mainboard.u2, parser::icd2::icd2());
            let work_ram = map_legacy_part(year_hint, &console.mainboard.u3, parser::ram::ram());
            let rom = map_legacy_part(year_hint, &console.mainboard.u4, parser::sgb_rom::sgb_rom());
            let cic = map_legacy_part(year_hint, &console.mainboard.u5, parser::cic::cic());
            let coil = map_legacy_part(year_hint, &console.mainboard.coil1, parser::coil::coil());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.xtal1,
                parser::crystal_20mihz::crystal_20mihz(),
            );
            let mainboard = LegacySgb2Mainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                letter_at_top_right: console.mainboard.letter_at_top_right.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                icd2,
                work_ram,
                rom,
                cic,
                coil,
                crystal,
            };

            let metadata = LegacySgb2Metadata {
                stamp: console.shell.stamp,
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "sgb2".to_string(),
                title: format!("Unit #{}", console.index),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("sgb2", &submissions)?;
    Ok(submissions)
}

fn process_cgb_submissions() -> Result<Vec<LegacyCgbSubmission>, Error> {
    use gbhwdb_backend::input::cgb::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/CGB").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: CgbConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year.or(Some(1998));
            let cpu = map_legacy_part(year_hint, &console.mainboard.u1, parser::cgb_soc::cgb_soc());
            let work_ram = map_legacy_part(year_hint, &console.mainboard.u2, parser::ram::ram());
            let amplifier =
                map_legacy_part(year_hint, &console.mainboard.u3, parser::mgb_amp::mgb_amp());
            let regulator =
                map_legacy_part(year_hint, &console.mainboard.u4, parser::cgb_reg::cgb_reg());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_8mihz::crystal_8mihz(),
            );
            let mainboard = LegacyCgbMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                jun: console.mainboard.jun,
                cpu,
                work_ram,
                amplifier,
                regulator,
                crystal,
            };

            let (old_stamp, new_stamp) = match &console.mainboard.stamp {
                Some(stamp) => {
                    if stamp.starts_with(&['6', '7', '8', '9'][..]) {
                        (
                            Some(
                                gbhwdb_backend::parser::dmg_stamp::dmg_stamp()
                                    .parse(&stamp)
                                    .unwrap_or_else(|_| panic!("{}", stamp)),
                            ),
                            None,
                        )
                    } else {
                        (
                            None,
                            Some(
                                gbhwdb_backend::parser::cgb_stamp::cgb_stamp()
                                    .parse(&stamp)
                                    .unwrap_or_else(|_| panic!("{}", stamp)),
                            ),
                        )
                    }
                }
                None => (None, None),
            };
            let stamp_year = new_stamp
                .as_ref()
                .and_then(|stamp| stamp.year)
                .or(old_stamp.as_ref().and_then(|stamp| stamp.year));

            let metadata = LegacyCgbMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                year: to_legacy_year(year_hint, stamp_year),
                month: old_stamp.as_ref().and_then(|stamp| stamp.month),
                week: new_stamp.as_ref().and_then(|stamp| stamp.week),
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "cgb".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("cgb", &submissions)?;
    Ok(submissions)
}

fn process_agb_submissions() -> Result<Vec<LegacyAgbSubmission>, Error> {
    use gbhwdb_backend::input::agb::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/AGB").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: AgbConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year.or(Some(2001));
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::agb_soc_qfp_128::agb_soc_qfp_128(),
            );
            let work_ram = map_legacy_part(
                year_hint,
                &console.mainboard.u2,
                parser::sram_tsop1_48::sram_tsop1_48(),
            );
            let regulator =
                map_legacy_part(year_hint, &console.mainboard.u3, parser::agb_reg::agb_reg());
            let u4 = map_legacy_part(
                year_hint,
                &console.mainboard.u4,
                parser::agb_pmic::agb_pmic(),
            );
            let amplifier =
                map_legacy_part(year_hint, &console.mainboard.u6, parser::agb_amp::agb_amp());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_4mihz::crystal_4mihz(),
            );
            let mainboard = LegacyAgbMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                work_ram,
                amplifier,
                regulator,
                crystal,
                u4,
            };

            let stamp = console.mainboard.stamp.as_ref().map(|stamp| {
                gbhwdb_backend::parser::cgb_stamp::cgb_stamp()
                    .parse(&stamp)
                    .unwrap_or_else(|_| panic!("{}", stamp))
            });

            let metadata = LegacyAgbMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                year: stamp
                    .as_ref()
                    .and_then(|stamp| to_legacy_year(year_hint, stamp.year)),
                week: stamp.as_ref().and_then(|stamp| stamp.week),
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "agb".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("agb", &submissions)?;
    Ok(submissions)
}

fn process_ags_submissions() -> Result<Vec<LegacyAgsSubmission>, Error> {
    use gbhwdb_backend::input::ags::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/AGS").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: AgsConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year.or(Some(2003));
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::agb_soc_qfp_156::agb_soc_qfp_156(),
            );
            let work_ram = map_legacy_part(
                year_hint,
                &console.mainboard.u2,
                parser::sram_tsop1_48::sram_tsop1_48(),
            );
            let amplifier = match console.mainboard.label.as_str() {
                // FIXME: Not really an amplifier
                "C/AGS-CPU-30" | "C/AGT-CPU-01" => map_legacy_part(
                    year_hint,
                    &console.mainboard.u3,
                    parser::ags_pmic_new::ags_pmic_new(),
                ),
                _ => map_legacy_part(year_hint, &console.mainboard.u3, parser::agb_amp::agb_amp()),
            };
            let u4 = map_legacy_part(
                year_hint,
                &console.mainboard.u4,
                parser::ags_pmic_old::ags_pmic_old(),
            );
            let u5 = map_legacy_part(
                year_hint,
                &console.mainboard.u5,
                parser::ags_charge_ctrl::ags_charge_ctrl(),
            );
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.x1,
                parser::crystal_4mihz::crystal_4mihz(),
            );
            let mainboard = LegacyAgsMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                work_ram,
                amplifier,
                u4,
                u5,
                crystal,
            };

            let metadata = LegacyAgsMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                mainboard,
            };

            let mut photos = LegacyAgsPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.top = get_photo(root, "02_top.jpg");
            photos.back = get_photo(root, "03_back.jpg");
            photos.pcb_front = get_photo(root, "04_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "05_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "ags".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("ags", &submissions)?;
    Ok(submissions)
}

fn process_gbs_submissions() -> Result<Vec<LegacyGbsSubmission>, Error> {
    use gbhwdb_backend::input::gbs::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/GBS").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: GbsConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );

            let year_hint = console.mainboard.year.or(Some(2003));
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u2,
                parser::agb_soc_qfp_128::agb_soc_qfp_128(),
            );
            let work_ram = map_legacy_part(
                year_hint,
                &console.mainboard.u3,
                parser::sram_tsop1_48::sram_tsop1_48(),
            );
            let u4 = map_legacy_part(year_hint, &console.mainboard.u4, parser::gbs_dol::gbs_dol());
            let u5 = map_legacy_part(year_hint, &console.mainboard.u5, parser::gbs_reg::gbs_reg());
            let u6 = map_legacy_part(year_hint, &console.mainboard.u6, parser::gbs_reg::gbs_reg());
            let crystal = map_legacy_part(
                year_hint,
                &console.mainboard.y1,
                parser::crystal_32mihz::crystal_32mihz(),
            );
            let mainboard = LegacyGbsMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                number_pair: console.mainboard.number_pair.clone(),
                stamp: console.mainboard.stamp.clone(),
                stamp_front: console.mainboard.stamp_front.clone(),
                stamp_back: console.mainboard.stamp_back.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                work_ram,
                crystal,
                u4,
                u5,
                u6,
            };

            let stamp = console.mainboard.stamp.as_ref().map(|stamp| {
                gbhwdb_backend::parser::cgb_stamp::cgb_stamp()
                    .parse(&stamp)
                    .unwrap_or_else(|_| panic!("{}", stamp))
            });

            let metadata = LegacyGbsMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                year: stamp
                    .as_ref()
                    .and_then(|stamp| to_legacy_year(year_hint, stamp.year)),
                week: stamp.as_ref().and_then(|stamp| stamp.week),
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "gbs".to_string(),
                title: format!("Unit #{}", console.index),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("gbs", &submissions)?;
    Ok(submissions)
}

fn process_oxy_submissions() -> Result<Vec<LegacyOxySubmission>, Error> {
    use gbhwdb_backend::input::oxy::*;
    use legacy::console::*;
    let walker = WalkDir::new("data/consoles/OXY").min_depth(2).max_depth(2);
    let mut submissions = Vec::new();
    for entry in walker.into_iter().filter_entry(is_metadata_file) {
        let entry = entry?;
        if let Some(root) = entry.path().parent() {
            debug!("{}", entry.path().display());
            let file = File::open(&entry.path())?;
            let console: OxyConsole = serde_json::from_reader(file)?;
            assert_eq!(
                Some(console.slug.as_str()),
                root.file_name().and_then(|name| name.to_str())
            );
            if let Some(serial) = &console.shell.serial {
                assert_eq!(&console.slug, serial);
            }

            let year_hint = console.mainboard.year.or(Some(2005));
            let cpu = map_legacy_part(
                year_hint,
                &console.mainboard.u1,
                parser::agb_soc_bga::agb_soc_bga(),
            );
            let u2 = map_legacy_part(
                year_hint,
                &console.mainboard.u2,
                parser::oxy_pmic::oxy_pmic(),
            );
            let u4 = map_legacy_part(year_hint, &console.mainboard.u4, parser::oxy_u4::oxy_u4());
            let u5 = map_legacy_part(year_hint, &console.mainboard.u5, parser::oxy_u5::oxy_u5());
            let mainboard = LegacyOxyMainboard {
                kind: console.mainboard.label.clone(),
                circled_letters: console.mainboard.circled_letters.clone(),
                year: console.mainboard.year,
                month: console.mainboard.month,
                cpu,
                u2,
                u4,
                u5,
            };

            let metadata = LegacyOxyMetadata {
                color: console.shell.color.map(|c| format!("{:?}", c)),
                release_code: console.shell.release_code.clone(),
                mainboard,
            };

            let mut photos = LegacyDefaultPhotos::default();
            photos.front = get_photo(root, "01_front.jpg");
            photos.back = get_photo(root, "02_back.jpg");
            photos.pcb_front = get_photo(root, "03_pcb_front.jpg");
            photos.pcb_back = get_photo(root, "04_pcb_back.jpg");
            submissions.push(LegacySubmission {
                code: "oxy".to_string(),
                title: console
                    .shell
                    .serial
                    .clone()
                    .unwrap_or_else(|| format!("Unit #{}", console.index.unwrap())),
                slug: console.slug,
                sort_group: None,
                contributor: console.contributor,
                metadata,
                photos,
            });
        }
    }
    submissions.sort_by_key(|submission| (submission.sort_group.clone(), submission.slug.clone()));
    write_console_submission_csv("oxy", &submissions)?;
    Ok(submissions)
}

fn copy_static_files() -> Result<(), Error> {
    static PATTERNS: [&str; 8] = [
        "site/static/**/*.html",
        "site/static/**/*.txt",
        "site/static/**/*.ico",
        "site/static/**/*.jpg",
        "site/static/**/*.png",
        "site/static/**/*.svg",
        "site/static/**/*.webmanifest",
        "site/static/**/*.xml",
    ];
    let target = Path::new("build");
    for pattern in &PATTERNS {
        for entry in glob(pattern)? {
            let path = entry?;
            let target = target.join(path.strip_prefix("site/static")?);
            if let Some(parent) = target.parent() {
                create_dir_all(parent)?;
            }
            fs::copy(&path, target)?;
        }
    }
    Ok(())
}
