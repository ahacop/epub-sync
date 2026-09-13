//! The EpubSync command line. Every command except `init` loads the config
//! and opens the library, which takes the lock for the length of the run.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use epubsync_core::config::{self, Config};
use epubsync_core::device::{Action, Device};
use epubsync_core::kobo::{self, Kobo};
use epubsync_core::library::{Book, ImportOutcome, Library};
use epubsync_core::metadata::{Author, Metadata, Series, format_series_number};
use epubsync_core::sort_name::sort_name;
use epubsync_core::sync as core_sync;

#[derive(Parser)]
#[command(
    name = "epubsync",
    version,
    about = "Manage a KEPUB library and sync it to a Kobo"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the library folder and its database, and point the config at it
    Init { folder: PathBuf },
    /// Convert an EPUB to KEPUB and add it to the library. A folder imports every EPUB in it
    Import {
        path: PathBuf,
        /// Import even when a book with the same title and first author exists
        #[arg(long)]
        force: bool,
    },
    /// List every book with its id, metadata, and progress per device
    List,
    /// Edit a book's metadata in $EDITOR, or one field per flag
    Edit {
        book: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        publisher: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// An author as "Name" or "Name|Sort name". Repeat for several. Replaces the list
        #[arg(long = "author")]
        authors: Vec<String>,
        #[arg(long)]
        series: Option<String>,
        #[arg(long)]
        series_number: Option<f64>,
    },
    /// Take a book out of the library
    Remove {
        book: i64,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Make the device folder match the library and read progress and words back
    Sync {
        /// Print the plan and change nothing
        #[arg(long)]
        dry_run: bool,
        /// The mounted device volume, instead of scanning the usual mount folders
        #[arg(long)]
        device: Option<PathBuf>,
        /// Write to the Kobo database on a firmware version the app has not been tested with
        #[arg(long)]
        allow_newer_firmware: bool,
        /// Run the deletes without asking
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// List the words looked up on the device, newest first
    Words {
        #[arg(long)]
        book: Option<i64>,
        /// A device serial
        #[arg(long)]
        device: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<()> {
    if let Command::Init { folder } = &command {
        let lib = Library::init(folder)?;
        println!("created the library at {}", lib.folder.display());
        println!("config written to {}", config::path()?.display());
        return Ok(());
    }
    let config = config::load()?;
    match command {
        Command::Init { .. } => unreachable!(),
        Command::Import { path, force } => import(&config, &path, force),
        Command::List => list(&config),
        Command::Edit {
            book,
            title,
            publisher,
            description,
            authors,
            series,
            series_number,
        } => {
            let flags = EditFlags {
                title,
                publisher,
                description,
                authors,
                series,
                series_number,
            };
            edit(&config, book, flags)
        }
        Command::Remove { book, yes } => remove(&config, book, yes),
        Command::Sync {
            dry_run,
            device,
            allow_newer_firmware,
            yes,
        } => sync(
            &config,
            SyncFlags {
                dry_run,
                device,
                allow_newer_firmware,
                yes,
            },
        ),
        Command::Words { .. } => bail!("words is not implemented yet"),
    }
}

fn import(config: &Config, path: &Path, force: bool) -> Result<()> {
    let mut lib = Library::open(config)?;
    let files = if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("read {}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".epub"))
            .collect();
        files.sort();
        files
    } else {
        vec![path.to_path_buf()]
    };
    if files.is_empty() {
        bail!("no .epub files in {}", path.display());
    }
    let mut failed = 0;
    for file in &files {
        match lib.import(file, force) {
            Ok(ImportOutcome::Imported { id, made_sort }) => {
                let book = lib.get(id)?;
                println!("{id:>5}  {}  {}", book.metadata.title, file.display());
                for author in made_sort {
                    println!("       made sort name for {}: {}", author.name, author.sort);
                }
            }
            Ok(ImportOutcome::Exists { id }) => {
                println!(
                    "{id:>5}  already in the library, skipped  {}",
                    file.display()
                );
            }
            Err(e) => {
                failed += 1;
                println!("error  {}: {e:#}", file.display());
            }
        }
    }
    if failed > 0 {
        bail!("{failed} of {} files failed", files.len());
    }
    Ok(())
}

fn list(config: &Config) -> Result<()> {
    let lib = Library::open(config)?;
    for book in lib.list()? {
        println!("{}", book_line(&book));
    }
    Ok(())
}

fn book_line(book: &Book) -> String {
    let m = &book.metadata;
    let authors: Vec<&str> = m.authors.iter().map(|a| a.name.as_str()).collect();
    let mut line = format!("{:>5}  {}  by {}", book.id, m.title, authors.join(" & "));
    if let Some(s) = &m.series {
        line.push_str(&format!("  [{}", s.name));
        if let Some(n) = s.number {
            line.push_str(&format!(" #{}", format_series_number(n)));
        }
        line.push(']');
    }
    line
}

struct EditFlags {
    title: Option<String>,
    publisher: Option<String>,
    description: Option<String>,
    authors: Vec<String>,
    series: Option<String>,
    series_number: Option<f64>,
}

impl EditFlags {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.publisher.is_none()
            && self.description.is_none()
            && self.authors.is_empty()
            && self.series.is_none()
            && self.series_number.is_none()
    }
}

fn edit(config: &Config, id: i64, flags: EditFlags) -> Result<()> {
    let mut lib = Library::open(config)?;
    let book = lib.get(id)?;
    let record = if flags.is_empty() {
        edit_in_editor(&book.metadata)?
    } else {
        apply_flags(book.metadata, flags)?
    };
    lib.edit(id, &record)?;
    println!("{}", book_line(&lib.get(id)?));
    Ok(())
}

fn apply_flags(mut record: Metadata, flags: EditFlags) -> Result<Metadata> {
    if let Some(t) = flags.title {
        record.title = t;
    }
    if let Some(p) = flags.publisher {
        record.publisher = Some(p).filter(|p| !p.is_empty());
    }
    if let Some(d) = flags.description {
        record.description = Some(d).filter(|d| !d.is_empty());
    }
    if !flags.authors.is_empty() {
        record.authors = flags
            .authors
            .iter()
            .map(|a| match a.split_once('|') {
                Some((name, sort)) => Author {
                    name: name.trim().to_string(),
                    sort: sort.trim().to_string(),
                },
                None => Author {
                    name: a.trim().to_string(),
                    sort: sort_name(a),
                },
            })
            .collect();
    }
    if let Some(name) = flags.series {
        if name.is_empty() {
            record.series = None;
        } else {
            let number = flags
                .series_number
                .or(record.series.as_ref().and_then(|s| s.number));
            record.series = Some(Series { name, number });
        }
    } else if let Some(n) = flags.series_number {
        match &mut record.series {
            Some(s) => s.number = Some(n),
            None => bail!("the book has no series. Pass --series with --series-number"),
        }
    }
    if record.title.trim().is_empty() {
        bail!("the title is empty");
    }
    Ok(record)
}

const EDIT_HEADER: &str = "# Edit the fields and save. An empty file cancels.
# authors is a list of tables with name and sort. series has name and number.
# Remove the series, publisher, or description block to clear the field.
# The description is written into the file as it is, HTML included.

";

fn edit_in_editor(record: &Metadata) -> Result<Metadata> {
    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|e| !e.trim().is_empty())
        .ok_or_else(|| anyhow!("set $EDITOR, or pass a flag such as --title"))?;
    let mut file = tempfile::Builder::new()
        .prefix("epubsync-")
        .suffix(".toml")
        .tempfile()?;
    let text = format!("{EDIT_HEADER}{}", toml::to_string_pretty(record)?);
    file.write_all(text.as_bytes())?;
    file.flush()?;

    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap();
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(file.path())
        .status()
        .with_context(|| format!("run {editor}"))?;
    if !status.success() {
        bail!("{editor} exited with {status}");
    }
    let edited = std::fs::read_to_string(file.path())?;
    let body: String = edited
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    if body.trim().is_empty() {
        bail!("empty file, nothing changed");
    }
    let record: Metadata = toml::from_str(&body).context("parse the edited TOML")?;
    if record.title.trim().is_empty() {
        bail!("the title is empty");
    }
    Ok(record)
}

fn remove(config: &Config, id: i64, yes: bool) -> Result<()> {
    let mut lib = Library::open(config)?;
    let book = lib.get(id)?;
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("pass --yes to remove without a prompt");
        }
        print!("remove {} \"{}\"? [y/N] ", book.id, book.metadata.title);
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("kept");
            return Ok(());
        }
    }
    lib.remove(id)?;
    println!("removed {} \"{}\"", book.id, book.metadata.title);
    Ok(())
}

struct SyncFlags {
    dry_run: bool,
    device: Option<PathBuf>,
    #[allow(dead_code)]
    allow_newer_firmware: bool,
    yes: bool,
}

fn sync(config: &Config, flags: SyncFlags) -> Result<()> {
    let mut lib = Library::open(config)?;
    let mut kobo = match &flags.device {
        Some(path) => Kobo::at(path)?,
        None => {
            let mut found = kobo::detect(&kobo::default_roots());
            match found.len() {
                0 => bail!("no Kobo found. Plug it in, or pass --device <path>"),
                1 => found.remove(0),
                _ => {
                    let roots: Vec<String> =
                        found.iter().map(|k| k.root.display().to_string()).collect();
                    bail!(
                        "more than one Kobo found: {}. Pass --device <path>",
                        roots.join(", ")
                    );
                }
            }
        }
    };
    println!("Kobo {} at {}", kobo.serial(), kobo.root.display());

    let mut actions = core_sync::plan(&lib, &kobo)?;
    if actions.is_empty() {
        println!("nothing to do");
    }
    for action in &actions {
        println!("{}", action_line(&lib, action)?);
    }
    if flags.dry_run {
        return Ok(());
    }

    let deletes = actions
        .iter()
        .filter(|a| matches!(a, Action::Delete { .. }))
        .count();
    if deletes > 0 && !flags.yes {
        if !std::io::stdin().is_terminal() {
            bail!("pass --yes to run the deletes without a prompt");
        }
        print!("delete {deletes} file(s) from the device? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("deletes skipped");
            actions.retain(|a| !matches!(a, Action::Delete { .. }));
        }
    }

    core_sync::apply(&mut lib, &mut kobo, &actions, |a| {
        let verb = match a {
            Action::Send { .. } => "sending",
            Action::Replace { .. } => "replacing",
            Action::SendAgain { .. } => "sending again",
            Action::Delete { .. } => "deleting",
        };
        println!("{verb} {}", a.id());
    })?;

    let back = core_sync::read_back(&mut lib, &mut kobo)?;
    if !back.progress.is_empty() {
        println!("read progress for {} book(s)", back.progress.len());
    }
    if !back.words.is_empty() {
        println!("{} new word(s)", back.words.len());
    }
    kobo.finish()?;
    println!("eject the device now");
    Ok(())
}

fn action_line(lib: &Library, action: &Action) -> Result<String> {
    let title = |id: i64| -> String { lib.get(id).map(|b| b.metadata.title).unwrap_or_default() };
    Ok(match action {
        Action::Send { id, .. } => format!("send        {id:>5}  {}", title(*id)),
        Action::Replace { id, .. } => format!("replace     {id:>5}  {}", title(*id)),
        Action::SendAgain { id, .. } => {
            format!("send again  {id:>5}  {}  (deleted on device)", title(*id))
        }
        Action::Delete { id } => format!("delete      {id:>5}  (no longer in the library)"),
    })
}
