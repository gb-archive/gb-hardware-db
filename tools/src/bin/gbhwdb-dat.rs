use anyhow::{anyhow, Error};
use cursive::{traits::*, view::Margins, views::*, Cursive, CursiveExt};
use gbhwdb_backend::config::cartridge::{BoardLayout, GameConfig, GamePlatform};
use gbhwdb_tools::{cursive::*, dat::DatFile};
use glob::glob;
use itertools::Itertools;
use std::{
    cell::Cell,
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
    fmt,
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    rc::Rc,
    sync::atomic::{self, AtomicBool},
};
use strsim::jaro;

#[derive(Clone, Debug)]
struct Dats {
    gb: DatFile,
    gbc: DatFile,
    gba: DatFile,
}

impl Dats {
    pub fn get_platform(&self, name: &str) -> Option<GamePlatform> {
        match (
            self.gb.names.contains(name),
            self.gbc.names.contains(name),
            self.gba.names.contains(name),
        ) {
            (true, false, false) => Some(GamePlatform::Gb),
            (false, true, false) => Some(GamePlatform::Gbc),
            (false, false, true) => Some(GamePlatform::Gba),
            _ => None,
        }
    }
    pub fn all_names(&self) -> HashSet<String> {
        self.gb
            .names
            .iter()
            .chain(self.gbc.names.iter())
            .chain(self.gba.names.iter())
            .cloned()
            .collect()
    }
    pub fn all_games(&self) -> Vec<(GamePlatform, String)> {
        let gb = self
            .gb
            .names
            .iter()
            .cloned()
            .map(|name| (GamePlatform::Gb, name));
        let gbc = self
            .gbc
            .names
            .iter()
            .cloned()
            .map(|name| (GamePlatform::Gbc, name));
        let gba = self
            .gba
            .names
            .iter()
            .cloned()
            .map(|name| (GamePlatform::Gba, name));
        gb.chain(gbc).chain(gba).collect()
    }
}

fn load_dats() -> Result<Dats, Error> {
    let mut gb_dat = None;
    let mut gbc_dat = None;
    let mut gba_dat = None;
    for entry in glob("*.dat")
        .expect("Invalid glob pattern")
        .filter_map(Result::ok)
    {
        match gbhwdb_tools::dat::from_path(&entry) {
            Ok(dat) => match dat.header.as_str() {
                "Nintendo - Game Boy" => gb_dat = Some(dat),
                "Nintendo - Game Boy Color" => gbc_dat = Some(dat),
                "Nintendo - Game Boy Advance" => gba_dat = Some(dat),
                _ => (),
            },
            Err(e) => eprintln!("Failed to read {}: {}", entry.to_string_lossy(), e),
        }
    }
    Ok(Dats {
        gb: gb_dat.ok_or(anyhow!("No GB dat found"))?,
        gbc: gbc_dat.ok_or(anyhow!("No GBC dat found"))?,
        gba: gba_dat.ok_or(anyhow!("No GBA dat found"))?,
    })
}

fn load_cfgs<P: AsRef<Path>>(path: P) -> Result<BTreeMap<String, GameConfig>, Error> {
    let file = File::open(path)?;
    let file = BufReader::new(file);
    let cfgs = serde_json::from_reader(file)?;
    Ok(cfgs)
}

fn write_cfgs<P: AsRef<Path>>(path: P, cfgs: &BTreeMap<String, GameConfig>) -> Result<(), Error> {
    let file = File::create(path)?;
    let file = BufWriter::new(file);
    serde_json::to_writer_pretty(file, cfgs)?;
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Command {
    Sync,
    Add,
    Quit,
}

fn main_menu(siv: &mut Cursive, cfgs: &BTreeMap<String, GameConfig>, dats: &Dats) -> Command {
    siv.add_layer(
        Dialog::new().title("gbhwdb-dat").content(
            LinearLayout::vertical()
                .child(TextView::new(format!(
                    " GB dat version: {}",
                    dats.gb.version
                )))
                .child(TextView::new(format!(
                    "GBC dat version: {}",
                    dats.gbc.version
                )))
                .child(TextView::new(format!(
                    "GBA dat version: {}",
                    dats.gba.version
                )))
                .child(TextView::new(format!("Game count: {}", cfgs.len())))
                .child(DummyView.fixed_height(1))
                .child(
                    SelectView::new()
                        .item("Synchronize", Command::Sync)
                        .item("Add a game", Command::Add)
                        .item("Quit", Command::Quit)
                        .on_submit(|s, _| s.quit())
                        .with_name("cmd"),
                ),
        ),
    );
    siv.run();
    let cmd = siv.get_select_view_selection::<Command>("cmd");
    siv.pop_layer();
    if should_quit() {
        Command::Quit
    } else {
        cmd.unwrap_or(Command::Quit)
    }
}

static QUIT: AtomicBool = AtomicBool::new(false);

fn should_quit() -> bool {
    QUIT.load(atomic::Ordering::SeqCst)
}

fn main() -> Result<(), Error> {
    let mut cfgs = load_cfgs("config/games.json")?;
    let dats = load_dats()?;
    let mut siv = Cursive::default();
    siv.add_global_callback('q', |s| {
        QUIT.store(true, atomic::Ordering::SeqCst);
        s.quit();
    });
    while !should_quit() {
        let cmd = main_menu(&mut siv, &cfgs, &dats);
        match cmd {
            Command::Sync => {
                sync(&mut siv, &mut cfgs, &dats);
                write_cfgs("config/games.json", &cfgs)?;
            }
            Command::Add => {
                add(&mut siv, &mut cfgs, &dats);
                write_cfgs("config/games.json", &cfgs)?;
            }
            Command::Quit => break,
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Candidate {
    platform: GamePlatform,
    name: String,
    rating: f64,
}

impl Candidate {
    pub fn new(platform: GamePlatform, current_name: &str, name: &str) -> Candidate {
        Candidate {
            platform,
            name: name.to_owned(),
            rating: jaro(&current_name.to_lowercase(), &name.to_lowercase()),
        }
    }
}

impl fmt::Display for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Score {:.02}: {} [{}]",
            self.rating, self.name, self.platform
        )
    }
}

fn sync(siv: &mut Cursive, cfgs: &mut BTreeMap<String, GameConfig>, dats: &Dats) {
    let names = dats.all_names();
    let games = dats.all_games();
    let name_problems = cfgs
        .iter_mut()
        .filter(|(_, game_cfg)| !names.contains(&game_cfg.name))
        .collect::<Vec<_>>();
    if name_problems.len() > 0 {
        let total = name_problems.len();
        for (idx, (code, game_cfg)) in name_problems.into_iter().enumerate() {
            let candidates = games
                .iter()
                .map(|(platform, name)| Candidate::new(*platform, &game_cfg.name, &name))
                .sorted_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(Ordering::Equal))
                .take(5)
                .map(|c| (format!("{}", c), Some(c)));
            let current_name = game_cfg.name.clone();
            siv.add_layer(
                Dialog::new()
                    .title(format!("Fix name problem {}/{}", idx + 1, total))
                    .padding(Margins::lrtb(2, 2, 1, 1))
                    .content(
                        LinearLayout::vertical()
                            .child(TextView::new(format!("Game code: {}", code)))
                            .child(TextView::new(format!("Before: {}", game_cfg.name)))
                            .child(
                                TextView::new(format!("After:  {}", game_cfg.name))
                                    .with_name("selection"),
                            )
                            .child(DummyView.fixed_height(1))
                            .child(
                                SelectView::new()
                                    .item("(skip)", None)
                                    .with_all(candidates)
                                    .on_submit(|s, _| s.quit())
                                    .on_select(move |s, selected| {
                                        let name = selected
                                            .as_ref()
                                            .map(|c| &c.name)
                                            .unwrap_or(&current_name);
                                        s.set_text_view_content(
                                            "selection",
                                            format!("After:  {}", name),
                                        );
                                    })
                                    .with_name("choice"),
                            ),
                    ),
            );
            siv.run();
            let choice = siv
                .get_select_view_selection::<Option<Candidate>>("choice")
                .and_then(|c| c);
            siv.pop_layer();
            if should_quit() {
                return;
            }
            if let Some(c) = choice {
                game_cfg.name = c.name;
            }
        }
    }

    let platform_problems = cfgs
        .iter_mut()
        .filter_map(|(code, cfg)| match dats.get_platform(&cfg.name) {
            Some(platform) if platform != cfg.platform => Some((code, cfg, platform)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if platform_problems.len() > 0 {
        let total = platform_problems.len();
        for (idx, (code, cfg, platform)) in platform_problems.into_iter().enumerate() {
            let choice = Rc::new(Cell::new(false));
            let mut dialog = Dialog::new()
                .title(format!("Fix platform problem {}/{}", idx + 1, total))
                .content(
                    LinearLayout::vertical()
                        .child(TextView::new(format!("Game code: {}", code)))
                        .child(TextView::new(format!("Before: {}", cfg.platform)))
                        .child(TextView::new(format!("After:  {}", platform))),
                );
            {
                let choice = choice.clone();
                dialog.add_button("Ok", move |s| {
                    choice.set(true);
                    s.quit();
                });
            }
            {
                let choice = choice.clone();
                dialog.add_button("Cancel", move |s| {
                    choice.set(false);
                    s.quit();
                });
            }
            siv.add_layer(dialog);
            siv.run();
            siv.pop_layer();
            if should_quit() {
                return;
            }
            if choice.get() {
                cfg.platform = platform;
            }
        }
    }

    siv.add_layer(
        Dialog::around(TextView::new("Synchronization complete")).button("Ok", |s| s.quit()),
    );
    siv.run();
    siv.pop_layer();
}

fn add(siv: &mut Cursive, cfgs: &mut BTreeMap<String, GameConfig>, dats: &Dats) {
    siv.add_layer(
        Dialog::new()
            .title("Enter game code")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("Code:"))
                    .child(EditView::new().on_submit(|s, _| s.quit()).with_name("code")),
            )
            .button("Ok", |s| s.quit())
            .fixed_width(70),
    );
    siv.run();
    let code = siv.get_edit_view_value("code");
    siv.pop_layer();
    if code.len() == 0 || cfgs.contains_key(&code) {
        return;
    }

    let games = dats.all_games();
    let mut search = EditView::new();
    search.set_on_edit(move |s, text, _| {
        s.call_on_name("search_results", |results: &mut SelectView<Candidate>| {
            results.clear();
            if text.len() > 0 {
                let candidates = games
                    .iter()
                    .map(|(platform, name)| Candidate::new(*platform, text, &name))
                    .sorted_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(Ordering::Equal))
                    .take(10)
                    .map(|c| (format!("{}", c), c));
                results.add_all(candidates);
            }
        })
        .unwrap();
    });
    siv.add_layer(
        Dialog::new()
            .title("Select game to add")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("Search:"))
                    .child(search)
                    .child(DummyView.fixed_height(1))
                    .child(TextView::new("Results:"))
                    .child(
                        SelectView::<Candidate>::new()
                            .on_submit(|s, _| s.quit())
                            .with_name("search_results")
                            .fixed_height(10),
                    ),
            )
            .fixed_width(150),
    );
    siv.run();
    let (platform, name) = siv
        .get_select_view_selection::<Candidate>("search_results")
        .map(|c| (c.platform, c.name))
        .unwrap_or((GamePlatform::Gb, String::new()));
    siv.pop_layer();
    if name.len() == 0 || should_quit() {
        return;
    }
    let mut layout_radio = RadioGroup::new();
    let default_layout = match platform {
        GamePlatform::Gb => BoardLayout::RomMapper,
        GamePlatform::Gbc => BoardLayout::RomMapperRam,
        GamePlatform::Gba => BoardLayout::RomMapper,
    };
    let layout_container = LinearLayout::vertical()
        .child(layout_radio.button(BoardLayout::Rom, "Rom"))
        .child({
            let mut button = layout_radio.button(BoardLayout::RomMapper, "Rom + mapper");
            if default_layout == BoardLayout::RomMapper {
                button = button.selected();
            }
            button
        })
        .child({
            let mut button = layout_radio.button(BoardLayout::RomMapperRam, "Rom + mapper + ram");
            if default_layout == BoardLayout::RomMapperRam {
                button = button.selected();
            }
            button
        })
        .child(layout_radio.button(
            BoardLayout::RomMapperRamXtal,
            "Rom + mapper + ram + crystal",
        ))
        .child(layout_radio.button(BoardLayout::Mbc2, "MBC2"))
        .child(layout_radio.button(BoardLayout::Mbc6, "MBC6"))
        .child(layout_radio.button(BoardLayout::Mbc7, "MBC7"))
        .child(layout_radio.button(BoardLayout::Type15, "Type 15 (MBC5 + dual ROM)"))
        .child(layout_radio.button(BoardLayout::Huc3, "HuC-3"))
        .child(layout_radio.button(BoardLayout::Tama, "Tamagotchi 3"));
    let mut dialog = Dialog::new().title("Add a game").content(
        LinearLayout::vertical()
            .child(TextView::new("Name:"))
            .child(TextView::new(name.as_str()))
            .child(TextView::new("Platform:"))
            .child(TextView::new(format!("{}", platform)))
            .child(TextView::new("Code:"))
            .child(TextView::new(code.as_str()))
            .child(TextView::new("Board layout:"))
            .child(layout_container),
    );
    dialog.add_button("Ok", move |s| s.quit());
    siv.add_layer(dialog.fixed_width(150));
    siv.run();
    let layout = layout_radio.selection();
    siv.pop_layer();
    if should_quit() {
        return;
    }
    cfgs.insert(
        code,
        GameConfig {
            name,
            platform,
            layouts: vec![*layout],
        },
    );
}